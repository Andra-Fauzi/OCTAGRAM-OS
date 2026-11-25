// octagram-cargo: the Cargo-side bootstrap driver for OCTAGRAM.
//
// This is NOT upstream Cargo and does not claim to be. Upstream Cargo is a
// Rust program that must be built for x86_64-unknown-octagram once a native
// rustc exists (M4.4). What this program provides is the other half of the
// M4.4 gate, written in C++ against the same libc++/libc that the toolchain
// will use:
//
//   * read a Cargo.toml manifest (package, workspace, deps, lib/bin targets)
//   * read Cargo.lock and validate that every resolved package is present
//   * resolve path dependencies into a build order
//   * fingerprint sources and rebuild only what changed
//   * invoke a rustc executable (native one when present) with the same
//     arguments Cargo would pass for a static OCTAGRAM build
//
// Offline by design: no registry access, no network, no git dependencies.
// A registry dependency without a vendored path is reported as unsupported
// rather than silently ignored.
//
// Build: make -C user build/octagram-cargo.elf
// Run in the guest: octagram-cargo plan|build|check|run|clean|metadata

#include <algorithm>
#include <cerrno>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <map>
#include <set>
#include <sstream>
#include <string>
#include <vector>

#include <sys/wait.h>
#include <unistd.h>

namespace fs = std::filesystem;

namespace {

constexpr const char* kVersion = "0.1.0";

// The whole toolchain is built with -fno-exceptions, so failures exit with a
// diagnostic on stderr instead of unwinding. Every caller treats fail() as a
// terminal error; there is nothing to clean up that the kernel would not.
[[noreturn]] void fail(const std::string& message)
{
    std::cerr << "octagram-cargo: error: " << message << "\n";
    std::exit(1);
}

// ---------------------------------------------------------------- utilities

std::string read_file(const fs::path& path)
{
    std::ifstream input(path, std::ios::in | std::ios::binary);
    if (!input.is_open()) {
        fail("cannot read " + path.string());
    }
    std::ostringstream buffer;
    buffer << input.rdbuf();
    return buffer.str();
}

void write_file(const fs::path& path, const std::string& contents)
{
    std::error_code error;
    if (!path.parent_path().empty()) {
        fs::create_directories(path.parent_path(), error);
    }
    std::ofstream output(path, std::ios::out | std::ios::trunc | std::ios::binary);
    if (!output.is_open()) {
        fail("cannot write " + path.string());
    }
    output << contents;
    if (!output.good()) {
        fail("short write to " + path.string());
    }
}

std::string trim(const std::string& text)
{
    size_t begin = 0;
    while (begin < text.size() && (text[begin] == ' ' || text[begin] == '\t' || text[begin] == '\r')) {
        ++begin;
    }
    size_t end = text.size();
    while (end > begin && (text[end - 1] == ' ' || text[end - 1] == '\t' || text[end - 1] == '\r')) {
        --end;
    }
    return text.substr(begin, end - begin);
}

bool starts_with(const std::string& text, const std::string& prefix)
{
    return text.size() >= prefix.size() && text.compare(0, prefix.size(), prefix) == 0;
}

bool ends_with(const std::string& text, const std::string& suffix)
{
    return text.size() >= suffix.size() &&
           text.compare(text.size() - suffix.size(), suffix.size(), suffix) == 0;
}

// Remove a TOML comment while respecting single and double quoted strings.
std::string strip_comment(const std::string& line)
{
    bool in_single = false;
    bool in_double = false;
    for (size_t index = 0; index < line.size(); ++index) {
        char character = line[index];
        if (character == '\'' && !in_double) {
            in_single = !in_single;
        } else if (character == '"' && !in_single) {
            bool escaped = index > 0 && line[index - 1] == '\\';
            if (!escaped) {
                in_double = !in_double;
            }
        } else if (character == '#' && !in_single && !in_double) {
            return line.substr(0, index);
        }
    }
    return line;
}

std::string unquote(const std::string& text)
{
    std::string value = trim(text);
    if (value.size() >= 2 && ((value.front() == '"' && value.back() == '"') ||
                              (value.front() == '\'' && value.back() == '\''))) {
        std::string inner = value.substr(1, value.size() - 2);
        std::string result;
        result.reserve(inner.size());
        for (size_t index = 0; index < inner.size(); ++index) {
            if (inner[index] == '\\' && index + 1 < inner.size()) {
                char next = inner[++index];
                switch (next) {
                case 'n':
                    result.push_back('\n');
                    break;
                case 't':
                    result.push_back('\t');
                    break;
                case '\\':
                    result.push_back('\\');
                    break;
                case '"':
                    result.push_back('"');
                    break;
                default:
                    result.push_back(next);
                    break;
                }
            } else {
                result.push_back(inner[index]);
            }
        }
        return result;
    }
    return value;
}

// Split the inside of an array or inline table on top-level commas.
std::vector<std::string> split_top_level(const std::string& text, char separator)
{
    std::vector<std::string> parts;
    std::string current;
    bool in_single = false;
    bool in_double = false;
    int depth = 0;
    for (char character : text) {
        if (character == '\'' && !in_double) {
            in_single = !in_single;
        } else if (character == '"' && !in_single) {
            in_double = !in_double;
        } else if (!in_single && !in_double) {
            if (character == '[' || character == '{') {
                ++depth;
            } else if (character == ']' || character == '}') {
                --depth;
            } else if (character == separator && depth == 0) {
                parts.push_back(trim(current));
                current.clear();
                continue;
            }
        }
        current.push_back(character);
    }
    if (!trim(current).empty()) {
        parts.push_back(trim(current));
    }
    return parts;
}

// --------------------------------------------------------------- manifest

struct Dependency {
    std::string name;
    std::string version;      // registry requirement, display only
    std::string path;         // relative to the manifest directory
    bool optional = false;
    std::vector<std::string> features;
};

struct Target {
    std::string name;
    std::string kind;  // "lib" or "bin"
    std::string path;  // relative to the manifest directory
    bool is_test = false;
};

struct Manifest {
    fs::path path;      // path to Cargo.toml
    fs::path directory; // parent directory
    std::string package_name;
    std::string version;
    std::string edition = "2018";
    std::string rust_version;
    std::vector<Target> targets;
    std::vector<Dependency> dependencies;
    std::vector<std::string> workspace_members;
    bool is_workspace_root = false;
};

// A deliberately small TOML reader: enough for Cargo manifests, no tables of
// tables beyond [[bin]]/[[package]] and no dotted keys deeper than one level.
struct TomlTable {
    std::string name;
    std::map<std::string, std::string> values; // key -> raw value text
};

class TomlReader {
public:
    explicit TomlReader(std::string text) : text_(std::move(text)) {}

    std::vector<TomlTable> parse() const
    {
        std::vector<TomlTable> tables;
        TomlTable current;
        current.name = "";
        std::istringstream stream(text_);
        std::string raw_line;
        while (std::getline(stream, raw_line)) {
            std::string line = trim(strip_comment(raw_line));
            if (line.empty()) {
                continue;
            }
            if (line.front() == '[') {
                bool array = starts_with(line, "[[");
                size_t open = array ? 2 : 1;
                size_t close = line.find(array ? "]]" : "]", open);
                if (close == std::string::npos) {
                    fail("unterminated table header: " + line);
                }
                tables.push_back(current);
                current = TomlTable{};
                current.name = trim(line.substr(open, close - open));
                if (array) {
                    current.name += "[]";
                }
                continue;
            }
            size_t equals = line.find('=');
            if (equals == std::string::npos) {
                fail("expected key = value, got: " + line);
            }
            std::string key = trim(line.substr(0, equals));
            std::string value = trim(line.substr(equals + 1));
            // Multi-line arrays: keep appending until brackets balance.
            int depth = 0;
            for (char character : value) {
                if (character == '[' || character == '{') {
                    ++depth;
                } else if (character == ']' || character == '}') {
                    --depth;
                }
            }
            while (depth > 0 && std::getline(stream, raw_line)) {
                std::string continuation = trim(strip_comment(raw_line));
                value += " " + continuation;
                for (char character : continuation) {
                    if (character == '[' || character == '{') {
                        ++depth;
                    } else if (character == ']' || character == '}') {
                        --depth;
                    }
                }
            }
            if (depth != 0) {
                fail("unbalanced brackets in value for key " + key);
            }
            current.values[key] = value;
        }
        tables.push_back(current);
        return tables;
    }

private:
    std::string text_;
};

std::vector<std::string> parse_string_array(const std::string& raw)
{
    std::string value = trim(raw);
    if (value.size() < 2 || value.front() != '[' || value.back() != ']') {
        // A single string is valid for members = "x" in older manifests.
        return {unquote(value)};
    }
    std::vector<std::string> items;
    for (const std::string& part : split_top_level(value.substr(1, value.size() - 2), ',')) {
        if (!part.empty()) {
            items.push_back(unquote(part));
        }
    }
    return items;
}

std::map<std::string, std::string> parse_inline_table(const std::string& raw)
{
    std::map<std::string, std::string> fields;
    std::string value = trim(raw);
    if (value.size() < 2 || value.front() != '{' || value.back() != '}') {
        return fields;
    }
    for (const std::string& part : split_top_level(value.substr(1, value.size() - 2), ',')) {
        size_t equals = part.find('=');
        if (equals == std::string::npos) {
            continue;
        }
        fields[trim(part.substr(0, equals))] = trim(part.substr(equals + 1));
    }
    return fields;
}

Dependency parse_dependency(const std::string& name, const std::string& raw)
{
    Dependency dependency;
    dependency.name = name;
    std::string value = trim(raw);
    if (!value.empty() && value.front() == '{') {
        std::map<std::string, std::string> fields = parse_inline_table(value);
        auto path = fields.find("path");
        if (path != fields.end()) {
            dependency.path = unquote(path->second);
        }
        auto version = fields.find("version");
        if (version != fields.end()) {
            dependency.version = unquote(version->second);
        }
        auto optional = fields.find("optional");
        if (optional != fields.end()) {
            dependency.optional = trim(optional->second) == "true";
        }
        auto features = fields.find("features");
        if (features != fields.end()) {
            dependency.features = parse_string_array(features->second);
        }
        auto git = fields.find("git");
        if (git != fields.end()) {
            fail("git dependency '" + name + "' is not supported offline");
        }
    } else if (!value.empty() && value.front() == '[') {
        fail("unsupported dependency form for '" + name + "'");
    } else {
        dependency.version = unquote(value);
    }
    return dependency;
}

Manifest load_manifest(const fs::path& manifest_path)
{
    Manifest manifest;
    manifest.path = fs::absolute(manifest_path);
    manifest.directory = manifest.path.parent_path();
    std::vector<TomlTable> tables = TomlReader(read_file(manifest.path)).parse();
    bool have_package = false;
    bool have_lib = false;
    for (const TomlTable& table : tables) {
        if (table.name == "package") {
            have_package = true;
            auto name = table.values.find("name");
            if (name == table.values.end()) {
                fail("Cargo.toml is missing [package] name");
            }
            manifest.package_name = unquote(name->second);
            auto version = table.values.find("version");
            if (version != table.values.end()) {
                manifest.version = unquote(version->second);
            }
            auto edition = table.values.find("edition");
            if (edition != table.values.end()) {
                manifest.edition = unquote(edition->second);
            }
            auto rust_version = table.values.find("rust-version");
            if (rust_version != table.values.end()) {
                manifest.rust_version = unquote(rust_version->second);
            }
        } else if (table.name == "workspace") {
            manifest.is_workspace_root = true;
            auto members = table.values.find("members");
            if (members != table.values.end()) {
                manifest.workspace_members = parse_string_array(members->second);
            }
        } else if (table.name == "lib") {
            have_lib = true;
            Target target;
            target.kind = "lib";
            auto name = table.values.find("name");
            target.name = name != table.values.end() ? unquote(name->second)
                                                     : manifest.package_name;
            auto path = table.values.find("path");
            target.path = path != table.values.end() ? unquote(path->second) : "src/lib.rs";
            if (std::find_if(manifest.targets.begin(), manifest.targets.end(),
                             [](const Target& existing) { return existing.kind == "lib"; }) ==
                manifest.targets.end()) {
                manifest.targets.push_back(target);
            }
        } else if (table.name == "bin[]") {
            Target target;
            target.kind = "bin";
            auto name = table.values.find("name");
            auto path = table.values.find("path");
            target.name = name != table.values.end() ? unquote(name->second) : manifest.package_name;
            target.path = path != table.values.end() ? unquote(path->second) : "src/main.rs";
            manifest.targets.push_back(target);
        } else if (table.name == "dependencies" || table.name == "dev-dependencies" ||
                   table.name == "build-dependencies") {
            for (const auto& entry : table.values) {
                manifest.dependencies.push_back(parse_dependency(entry.first, entry.second));
            }
        }
    }

    if (!have_package && !manifest.is_workspace_root) {
        fail("Cargo.toml has neither [package] nor [workspace]");
    }
    if (!manifest.targets.empty() && !have_lib) {
        // [[bin]] entries were declared without a [lib]; nothing to add.
    }
    if (manifest.targets.empty() && have_package) {
        Target binary;
        binary.kind = "bin";
        binary.name = manifest.package_name;
        binary.path = "src/main.rs";
        manifest.targets.push_back(binary);
    }
    return manifest;
}

// --------------------------------------------------------------- lockfile

struct LockedPackage {
    std::string name;
    std::string version;
    std::string source;
};

std::vector<LockedPackage> load_lockfile(const fs::path& path)
{
    std::vector<LockedPackage> packages;
    if (!fs::exists(path)) {
        return packages;
    }
    std::vector<TomlTable> tables = TomlReader(read_file(path)).parse();
    for (const TomlTable& table : tables) {
        if (table.name != "package[]") {
            continue;
        }
        LockedPackage package;
        auto name = table.values.find("name");
        auto version = table.values.find("version");
        if (name == table.values.end() || version == table.values.end()) {
            fail("Cargo.lock [[package]] entry without name/version");
        }
        package.name = unquote(name->second);
        package.version = unquote(version->second);
        auto source = table.values.find("source");
        if (source != table.values.end()) {
            package.source = unquote(source->second);
        }
        packages.push_back(package);
    }
    return packages;
}

// ------------------------------------------------------------- build graph

struct Package {
    Manifest manifest;
    fs::path output_directory;  // assigned during planning
    std::vector<std::string> dependencies; // package names in build order
};

class Workspace {
public:
    void load(const fs::path& manifest_path)
    {
        fs::path absolute = fs::absolute(manifest_path);
        if (!fs::exists(absolute)) {
            fail("manifest not found: " + absolute.string());
        }
        root_ = add_package(absolute);
        const Manifest& root = packages_[root_].manifest;
        if (!root.is_workspace_root) {
            order_.push_back(packages_[root_].manifest);
            return;
        }
        std::vector<std::string> queue = root.workspace_members;
        if (!root.package_name.empty()) {
            queue.push_back(".");
        }
        for (const std::string& member : queue) {
            fs::path member_manifest = root.directory / member / "Cargo.toml";
            if (!fs::exists(member_manifest)) {
                fail("workspace member without Cargo.toml: " + member);
            }
            add_package(member_manifest);
        }
        compute_order(root_);
    }

    const std::vector<Manifest>& order() const { return order_; }
    const std::vector<Package>& packages() const { return packages_; }
    const Manifest& root_manifest() const { return packages_[root_].manifest; }

private:
    size_t add_package(const fs::path& manifest_path)
    {
        fs::path canonical = fs::weakly_canonical(manifest_path);
        auto existing = package_index_.find(canonical.string());
        if (existing != package_index_.end()) {
            return existing->second;
        }
        Package package;
        package.manifest = load_manifest(canonical);
        size_t index = packages_.size();
        packages_.push_back(package);
        package_index_[canonical.string()] = index;
        return index;
    }

    void visit(size_t index, std::set<size_t>& visiting, std::set<size_t>& done)
    {
        if (done.count(index) != 0) {
            return;
        }
        if (visiting.count(index) != 0) {
            fail("path dependency cycle involving " + packages_[index].manifest.package_name);
        }
        visiting.insert(index);
        // Copy the dependency list: add_package() grows packages_ and would
        // invalidate any reference or iterator into it.
        std::vector<Dependency> dependencies = packages_[index].manifest.dependencies;
        const fs::path directory = packages_[index].manifest.directory;
        for (const Dependency& dependency : dependencies) {
            if (dependency.path.empty()) {
                if (!dependency.version.empty() && dependency.optional) {
                    continue;
                }
                fail("dependency '" + dependency.name +
                     "' has no path; OCTAGRAM bootstrap builds are vendored/offline only");
            }
            fs::path dependency_manifest = directory / dependency.path / "Cargo.toml";
            if (!fs::exists(dependency_manifest)) {
                fail("path dependency '" + dependency.name + "' missing " +
                     dependency_manifest.string());
            }
            size_t dependency_index = add_package(dependency_manifest);
            visit(dependency_index, visiting, done);
            const std::string resolved_name = packages_[dependency_index].manifest.package_name;
            packages_[index].dependencies.push_back(resolved_name);
        }
        visiting.erase(index);
        done.insert(index);
        order_.push_back(packages_[index].manifest);
    }

    void compute_order(size_t root)
    {
        std::set<size_t> visiting;
        std::set<size_t> done;
        // Members first so the workspace root is built last.
        for (size_t index = 0; index < packages_.size(); ++index) {
            visit(index, visiting, done);
        }
        (void)root;
    }

    size_t root_ = 0;
    std::vector<Package> packages_;
    std::map<std::string, size_t> package_index_;
    std::vector<Manifest> order_;
};

// ------------------------------------------------------------- fingerprint

long long newest_source_time(const fs::path& directory)
{
    std::error_code error;
    if (!fs::exists(directory, error)) {
        return 0;
    }
    long long newest = 0;
    for (const fs::directory_entry& entry : fs::recursive_directory_iterator(directory, error)) {
        if (error) {
            break;
        }
        if (!entry.is_regular_file(error)) {
            continue;
        }
        const std::string name = entry.path().filename().string();
        if (name.size() > 4 && name.compare(name.size() - 4, 4, ".rs") != 0) {
            continue;
        }
        auto stamp = entry.last_write_time(error);
        if (error) {
            continue;
        }
        long long seconds =
            (long long)std::chrono::duration_cast<std::chrono::seconds>(stamp.time_since_epoch())
                .count();
        newest = std::max(newest, seconds);
    }
    return newest;
}

long long file_time(const fs::path& path)
{
    std::error_code error;
    if (!fs::exists(path, error)) {
        return 0;
    }
    auto stamp = fs::last_write_time(path, error);
    if (error) {
        return 0;
    }
    return (long long)std::chrono::duration_cast<std::chrono::seconds>(stamp.time_since_epoch())
        .count();
}

// ------------------------------------------------------------- rustc driving

int run_process(const std::vector<std::string>& arguments)
{
    std::vector<char*> argv;
    argv.reserve(arguments.size() + 1);
    for (const std::string& argument : arguments) {
        argv.push_back(const_cast<char*>(argument.c_str()));
    }
    argv.push_back(nullptr);

    pid_t child = fork();
    if (child < 0) {
        fail("fork failed");
    }
    if (child == 0) {
        execvp(argv[0], argv.data());
        const std::string message =
            std::string("octagram-cargo: cannot execute ") + argv[0] + "\n";
        (void)!write(2, message.c_str(), message.size());
        _exit(127);
    }
    int status = 0;
    while (waitpid(child, &status, 0) < 0) {
        if (errno != EINTR) {
            fail("waitpid failed");
        }
    }
    if (WIFEXITED(status)) {
        return WEXITSTATUS(status);
    }
    return 128 + (WIFSIGNALED(status) ? WTERMSIG(status) : 0);
}

struct Options {
    fs::path manifest_path = "Cargo.toml";
    fs::path target_directory = "target";
    std::string profile = "dev";
    std::string rustc = "rustc";
    std::string linker;
    bool offline = true;   // always true: no registry support
    bool locked = false;
    bool verbose = false;
    // `cargo run` selection: --bin NAME, plus everything after `--`.
    std::string bin;
    std::vector<std::string> run_arguments;
    std::vector<std::string> cargo_arguments;
};

std::string rustc_from_environment()
{
    const char* rustc = std::getenv("RUSTC");
    if (rustc != nullptr && rustc[0] != '\0') {
        return rustc;
    }
    const char* toolchain = std::getenv("OCTAGRAM_RUSTC");
    if (toolchain != nullptr && toolchain[0] != '\0') {
        return toolchain;
    }
    // /toolchain/bin is on the shell PATH in OCTAGRAM; fall back to the name so
    // a host-side dry run can use its own rustc.
    return "rustc";
}

// The builtin target names the host-side GCC driver as its linker, and an
// OCTAGRAM image does not carry it. Prefer a native linker installed with the
// toolchain (or in /bin), and fall back to TinyCC, the only executable linker
// the base image ships. This is what makes a future native ld drop-in work
// without recompiling the driver.
std::string select_linker()
{
    const char* configured = std::getenv("OCTAGRAM_RUST_LINKER");
    if (configured != nullptr && configured[0] != '\0') {
        return configured;
    }
    for (const char* candidate : {"/toolchain/bin/ld", "/bin/ld", "/usr/bin/ld"}) {
        std::error_code error;
        if (fs::exists(candidate, error)) {
            return candidate;
        }
    }
    return "tcc";
}

std::string target_output_name(const Target& target)
{
    if (target.kind == "lib") {
        return "lib" + target.name + ".rlib";
    }
    return target.name;
}

std::string edition_argument(const std::string& edition)
{
    if (edition.empty()) {
        return "2015";
    }
    return edition;
}

// -------------------------------------------------------------- commands

void print_plan(const Workspace& workspace)
{
    std::cout << "packages (" << workspace.order().size() << ")\n";
    for (const Manifest& manifest : workspace.order()) {
        std::cout << "  " << manifest.package_name;
        if (!manifest.version.empty()) {
            std::cout << " " << manifest.version;
        }
        std::cout << "  [" << manifest.path.string() << "]\n";
        for (const Target& target : manifest.targets) {
            std::cout << "    " << target.kind << " " << target.name << " -> " << target.path << "\n";
        }
        for (const Dependency& dependency : manifest.dependencies) {
            std::cout << "    dep " << dependency.name;
            if (!dependency.path.empty()) {
                std::cout << " (path " << dependency.path << ")";
            } else if (!dependency.version.empty()) {
                std::cout << " (version " << dependency.version << ")";
            }
            std::cout << "\n";
        }
    }
}

struct LibraryOutput {
    std::string crate_name; // rustc --crate-name and --extern name
    fs::path path;          // produced rlib
};

struct BuildState {
    // Keyed by package name: Cargo.toml dependency keys are package names,
    // while rustc wants the library crate name, which [lib] name can rename and
    // which never contains hyphens (package `demo-lib` is crate `demo_lib`).
    std::map<std::string, LibraryOutput> libraries;
};

bool sources_are_current(const Manifest& manifest, const fs::path& output)
{
    long long output_time = file_time(output);
    if (output_time == 0) {
        return false;
    }
    long long manifest_time = file_time(manifest.path);
    if (manifest_time > output_time) {
        return false;
    }
    return newest_source_time(manifest.directory / "src") <= output_time;
}

int build_workspace(const Workspace& workspace, const Options& options, bool check_only)
{
    BuildState state;
    fs::path profile_directory = fs::absolute(options.target_directory) / options.profile;
    std::error_code error;
    fs::create_directories(profile_directory, error);
    if (error) {
        fail("cannot create target directory: " + error.message());
    }
    int built = 0;
    int skipped = 0;

    for (const Manifest& manifest : workspace.order()) {
        for (const Target& target : manifest.targets) {
            fs::path source = manifest.directory / target.path;
            if (!fs::exists(source)) {
                fail("target source missing: " + source.string());
            }
            fs::path output = profile_directory / target_output_name(target);
            std::vector<std::string> arguments;
            arguments.push_back(options.rustc);
            arguments.push_back("--crate-name");
            arguments.push_back(target.name);
            arguments.push_back("--edition");
            arguments.push_back(edition_argument(manifest.edition));
            arguments.push_back("--crate-type");
            arguments.push_back(target.kind == "lib" ? "lib" : "bin");
            if (options.profile == "release") {
                arguments.push_back("-C");
                arguments.push_back("opt-level=3");
            } else {
                arguments.push_back("-C");
                arguments.push_back("opt-level=0");
            }
            arguments.push_back("-C");
            arguments.push_back("panic=abort");
            arguments.push_back("--target");
            const char* target_triple = std::getenv("OCTAGRAM_RUST_TARGET");
            arguments.push_back(target_triple != nullptr && target_triple[0] != '\0'
                                   ? target_triple
                                   : "x86_64-unknown-octagram");
            // Overrides the target's host-only linker; see select_linker().
            arguments.push_back("-C");
            arguments.push_back("linker=" + options.linker);
            for (const Dependency& dependency : manifest.dependencies) {
                auto found = state.libraries.find(dependency.name);
                if (found == state.libraries.end()) {
                    // A dependency that produced no library output (for
                    // example a binary-only path dependency) has no rlib to
                    // pass, which matches what rustc would need anyway.
                    continue;
                }
                arguments.push_back("--extern");
                arguments.push_back(found->second.crate_name + "=" +
                                    found->second.path.string());
            }
            arguments.push_back("-L");
            arguments.push_back("dependency=" + profile_directory.string());
            if (check_only) {
                arguments.push_back("--emit=metadata");
            } else {
                arguments.push_back("-o");
                arguments.push_back(output.string());
            }
            arguments.push_back(source.string());

            if (sources_are_current(manifest, output) && !check_only) {
                ++skipped;
                if (options.verbose) {
                    std::cout << "fresh " << target.name << "\n";
                }
            } else {
                if (options.verbose) {
                    std::cout << "rustc";
                    for (const std::string& argument : arguments) {
                        std::cout << " " << argument;
                    }
                    std::cout << "\n";
                }
                int status = run_process(arguments);
                if (status != 0) {
                    std::string message = "rustc failed for " + target.name + " (status " +
                                          std::to_string(status) + ", linker " +
                                          options.linker + ")";
                    if (options.linker == "tcc") {
                        message += "; if the linker is at fault, set OCTAGRAM_RUST_LINKER";
                    }
                    fail(message);
                }
                ++built;
            }
            if (target.kind == "lib") {
                state.libraries[manifest.package_name] = LibraryOutput{target.name, output};
            }
        }
    }
    std::cout << "built " << built << ", fresh " << skipped << "\n";
    return 0;
}

// `run` is the shell workflow `cargo build` alone cannot give: compile with
// the native rustc, then replace this process with the produced binary so the
// program owns the terminal, its exit status, and any arguments after `--`.
int run_target(const Workspace& workspace, const Options& options)
{
    int status = build_workspace(workspace, options, false);
    if (status != 0) {
        return status;
    }

    const Target* selected = nullptr;
    for (const Manifest& manifest : workspace.order()) {
        for (const Target& target : manifest.targets) {
            if (target.kind != "bin") {
                continue;
            }
            if (!options.bin.empty() && target.name != options.bin) {
                continue;
            }
            if (selected != nullptr) {
                fail("multiple binary targets; pass --bin NAME");
            }
            selected = &target;
        }
    }
    if (selected == nullptr) {
        if (options.bin.empty()) {
            fail("no binary target in this workspace");
        }
        fail("no binary target named " + options.bin);
    }

    fs::path executable =
        fs::absolute(options.target_directory) / options.profile / target_output_name(*selected);
    std::error_code error;
    if (!fs::exists(executable, error)) {
        fail("built binary is missing: " + executable.string());
    }

    std::vector<std::string> arguments;
    arguments.push_back(executable.string());
    for (const std::string& argument : options.run_arguments) {
        arguments.push_back(argument);
    }
    std::cout << "running " << executable.string() << "\n";
    return run_process(arguments);
}

// `new`/`init` are what make the guest usable without a host: the shell can
// create a package, edit it, and build it with the native toolchain. The
// generated manifest, crate name and Cargo.lock stay inside the supported
// subset (single binary, path dependencies only).
std::string sanitized_crate_name(const std::string& text)
{
    std::string sanitized;
    for (char character : text) {
        const bool allowed = (character >= 'a' && character <= 'z') ||
                             (character >= 'A' && character <= 'Z') ||
                             (character >= '0' && character <= '9') || character == '_';
        sanitized.push_back(allowed ? character : '_');
    }
    if (sanitized.empty() || (sanitized[0] >= '0' && sanitized[0] <= '9')) {
        sanitized.insert(sanitized.begin(), 'a');
    }
    return sanitized;
}

int create_project(const Options& options, bool initialize)
{
    if (options.cargo_arguments.size() > 1) {
        fail(std::string("usage: cargo ") + (initialize ? "init" : "new") + " [PATH]");
    }
    fs::path directory =
        options.cargo_arguments.empty() ? fs::path(".") : fs::path(options.cargo_arguments.front());
    std::error_code error;

    if (initialize) {
        if (!fs::is_directory(directory, error)) {
            fail("not a directory: " + directory.string());
        }
    } else {
        if (fs::exists(directory, error) &&
            (!fs::is_directory(directory, error) || !fs::is_empty(directory, error))) {
            fail("destination already exists and is not empty: " + directory.string());
        }
        fs::create_directories(directory, error);
        if (error) {
            fail("cannot create " + directory.string() + ": " + error.message());
        }
    }

    fs::path manifest = directory / "Cargo.toml";
    if (fs::exists(manifest, error)) {
        fail("Cargo.toml already exists in " + fs::absolute(directory).string());
    }

    std::string package_name = fs::absolute(directory).filename().string();
    if (package_name.empty() || package_name == "." || package_name == "..") {
        package_name = "octagram-app";
    }
    package_name = sanitized_crate_name(package_name);

    write_file(manifest, "[package]\nname = \"" + package_name +
                             "\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[[bin]]\nname = \"" +
                             package_name + "\"\npath = \"src/main.rs\"\n");
    write_file(directory / "src" / "main.rs",
               "fn main() {\n    println!(\"Hello from OCTAGRAM!\");\n}\n");
    // Written up front so `--locked` works on the first build; the driver never
    // rewrites it (only `build` outside --locked could, and it does not).
    write_file(directory / "Cargo.lock",
               "# Generated by octagram-cargo.\nversion = 4\n\n[[package]]\nname = \"" +
                   package_name + "\"\nversion = \"0.1.0\"\n");

    std::cout << (initialize ? "initialized " : "created ")
              << fs::absolute(directory).string() << "\n";
    return 0;
}

void clean(const Options& options)
{
    std::error_code error;
    fs::path target = fs::absolute(options.target_directory);
    if (fs::exists(target, error)) {
        fs::remove_all(target, error);
        if (error) {
            fail("cannot remove target directory: " + error.message());
        }
        std::cout << "removed " << target.string() << "\n";
    } else {
        std::cout << "nothing to clean\n";
    }
}

void print_metadata(const Workspace& workspace, const Options& options)
{
    std::cout << "{\n  \"packages\": [\n";
    bool first = true;
    for (const Manifest& manifest : workspace.order()) {
        if (!first) {
            std::cout << ",\n";
        }
        first = false;
        std::cout << "    {\"name\": \"" << manifest.package_name << "\", \"version\": \""
                  << manifest.version << "\", \"manifest_path\": \"" << manifest.path.string()
                  << "\", \"targets\": [";
        bool first_target = true;
        for (const Target& target : manifest.targets) {
            if (!first_target) {
                std::cout << ", ";
            }
            first_target = false;
            std::cout << "{\"name\": \"" << target.name << "\", \"kind\": [\"" << target.kind
                      << "\"], \"src_path\": \"" << (manifest.directory / target.path).string()
                      << "\"}";
        }
        std::cout << "]}";
    }
    std::cout << "\n  ],\n  \"target_directory\": \""
              << fs::absolute(options.target_directory).string() << "\"\n}\n";
}

Options parse_options(const std::vector<std::string>& arguments)
{
    Options options;
    options.rustc = rustc_from_environment();
    options.linker = select_linker();
    for (size_t index = 0; index < arguments.size(); ++index) {
        const std::string& argument = arguments[index];
        auto next = [&](const char* name) -> std::string {
            if (index + 1 >= arguments.size()) {
                fail(std::string("missing value for ") + name);
            }
            return arguments[++index];
        };
        if (argument == "--manifest-path") {
            options.manifest_path = next("--manifest-path");
        } else if (argument == "--target-dir") {
            options.target_directory = next("--target-dir");
        } else if (argument == "--release") {
            options.profile = "release";
        } else if (argument == "--profile") {
            options.profile = next("--profile");
        } else if (argument == "--rustc") {
            options.rustc = next("--rustc");
        } else if (argument == "--offline") {
            options.offline = true;
        } else if (argument == "--locked") {
            options.locked = true;
        } else if (argument == "--bin") {
            options.bin = next("--bin");
        } else if (argument == "--") {
            // Everything after `--` belongs to the program, not to the driver.
            for (size_t rest = index + 1; rest < arguments.size(); ++rest) {
                options.run_arguments.push_back(arguments[rest]);
            }
            break;
        } else if (argument == "--verbose" || argument == "-v") {
            options.verbose = true;
        } else if (argument == "--help" || argument == "-h") {
            options.cargo_arguments.push_back("--help");
        } else if (starts_with(argument, "-")) {
            fail("unsupported option: " + argument);
        } else {
            options.cargo_arguments.push_back(argument);
        }
    }
    return options;
}

void usage()
{
    std::cout << "octagram-cargo " << kVersion << " (Cargo bootstrap driver, not upstream Cargo)\n\n"
              << "usage: octagram-cargo <command> [options]\n\n"
              << "commands:\n"
              << "  plan      resolve the workspace and print the build order\n"
              << "  build     compile every target with rustc\n"
              << "  run       build, then execute the binary target\n"
              << "  check     type-check every target (--emit=metadata)\n"
              << "  clean     remove the target directory\n"
              << "  new PATH  create a binary package to build with the native rustc\n"
              << "  init PATH same as new, in an existing directory (default .)\n"
              << "  metadata  print a minimal JSON workspace description\n"
              << "  version   print the driver version\n\n"
              << "options:\n"
              << "  --manifest-path PATH   default Cargo.toml\n"
              << "  --target-dir PATH      default target\n"
              << "  --profile NAME         dev or release\n"
              << "  --release              shorthand for --profile release\n"
              << "  --rustc PATH           rustc executable (default $RUSTC or rustc)\n"
              << "  --bin NAME             binary target to run (default: the only one)\n"
              << "  --locked               require Cargo.lock to cover every package\n"
              << "  --offline              accepted; registry access is never used\n"
              << "  -v, --verbose          print rustc command lines\n"
              << "  -- ARGS...             arguments passed to the program by run\n";
}

int run(int argc, char** argv)
{
    if (argc < 2) {
        usage();
        return 2;
    }
    std::string command = argv[1];
    std::vector<std::string> arguments(argv + 2, argv + argc);
    Options options = parse_options(arguments);

    if (command == "version" || command == "--version" || command == "-V") {
        std::cout << "octagram-cargo " << kVersion << "\n";
        return 0;
    }
    if (command == "help" || command == "--help" || command == "-h") {
        usage();
        return 0;
    }
    if (command == "clean") {
        clean(options);
        return 0;
    }
    if (command == "new") {
        return create_project(options, false);
    }
    if (command == "init") {
        return create_project(options, true);
    }

    Workspace workspace;
    workspace.load(options.manifest_path);

    if (options.locked) {
        std::vector<LockedPackage> locked =
            load_lockfile(workspace.root_manifest().directory / "Cargo.lock");
        for (const Manifest& manifest : workspace.order()) {
            bool found = std::any_of(
                locked.begin(), locked.end(),
                [&](const LockedPackage& package) {
                    return package.name == manifest.package_name &&
                           (manifest.version.empty() || package.version == manifest.version);
                });
            if (!found) {
                fail("--locked given but Cargo.lock is missing " + manifest.package_name);
            }
        }
    }

    if (command == "plan") {
        print_plan(workspace);
        return 0;
    }
    if (command == "metadata") {
        print_metadata(workspace, options);
        return 0;
    }
    if (command == "build") {
        return build_workspace(workspace, options, false);
    }
    if (command == "check") {
        return build_workspace(workspace, options, true);
    }
    if (command == "run") {
        return run_target(workspace, options);
    }
    fail("unknown command: " + command);
}

} // namespace

int main(int argc, char** argv) { return run(argc, argv); }
