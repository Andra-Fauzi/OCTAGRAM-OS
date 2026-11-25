// Guest probe for <format>, <print>, <expected>, <syncstream> and mdspan.
//
// These are the pieces a compiler front end uses to build diagnostics and to
// emit structured output: std::format for messages, std::format_to into a
// buffer, std::vformat for runtime format strings, std::print/std::println for
// unbuffered writes, and std::osyncstream for thread-safe logging.
//
// Prints LIBCXX-FORMAT-PASS only when every check succeeded.

#include <cstdio>
#include <expected>
#include <format>
#include <mdspan>
#include <print>
#include <sstream>
#include <string>
#include <syncstream>
#include <thread>
#include <vector>

namespace {

int failures = 0;

void check(bool condition, const char* what)
{
    if (!condition) {
        std::println("FAIL {}", what);
        ++failures;
    }
}

void check_text(const std::string& actual, const std::string& expected, const char* what)
{
    if (actual != expected) {
        std::println("FAIL {}: got '{}' want '{}'", what, actual, expected);
        ++failures;
    }
}

void check_integers()
{
    check_text(std::format("{}", 42), "42", "int default");
    check_text(std::format("{}", -7), "-7", "negative int");
    check_text(std::format("{:d}", 1234567), "1234567", "decimal specifier");
    check_text(std::format("{:x}", 0xdeadbeefu), "deadbeef", "hex");
    check_text(std::format("{:#x}", 0xdeadbeefu), "0xdeadbeef", "hex with prefix");
    check_text(std::format("{:X}", 0xbeefu), "BEEF", "upper hex");
    check_text(std::format("{:o}", 0755u), "755", "octal");
    check_text(std::format("{:b}", 0b1010u), "1010", "binary");
    check_text(std::format("{:08}", 42), "00000042", "zero fill");
    check_text(std::format("{:*^9}", 42), "***42****", "centered fill");
    check_text(std::format("{:>6}|", 42), "    42|", "right align");
    check_text(std::format("{:<6}|", 42), "42    |", "left align");
    check_text(std::format("{:+#010x}", 0x2au), "+0x000002a", "flags combined");
    check_text(std::format("{}", true), "true", "bool");
    check_text(std::format("{}", 'A'), "A", "char");
    check_text(std::format("{:c}", 'z'), "z", "char specifier");
}

void check_strings()
{
    check_text(std::format("{}", "octagram"), "octagram", "string literal");
    check_text(std::format("{}", std::string("native")), "native", "std::string");
    check_text(std::format("{:>10}", std::string("self")), "      self", "string align");
    check_text(std::format("{:.4}", std::string("self-hosting")), "self", "string precision");
    check_text(std::format("{} {}", 1, std::string("two")), "1 two", "two arguments");
    check_text(std::format("{{{}}}", 5), "{5}", "escaped braces");

    char wide_buffer[8];
    int written = std::snprintf(wide_buffer, sizeof(wide_buffer), "%s", "ok");
    check(written == 2, "snprintf still works alongside format");
}

void check_floats()
{
    check_text(std::format("{:.1f}", 3.14159), "3.1", "fixed precision");
    check_text(std::format("{:.3f}", 2.0), "2.000", "fixed trailing zeros");
    check_text(std::format("{:e}", 1234.5), "1.234500e+03", "scientific");
    check_text(std::format("{:.2e}", 0.000125), "1.25e-04", "scientific precision");
    check_text(std::format("{}", 1.5), "1.5", "general");
    check_text(std::format("{:10.2f}|", 3.5), "      3.50|", "float width");
    check_text(std::format("{:.2}", 1234.0), "1.2e+03", "general precision");
    check_text(std::format("{:g}", 100000.0), "100000", "shortest g");
    check_text(std::format("{}", 0.5f), "0.5", "float");
}

struct Version {
    int major;
    int minor;
};

} // namespace

template <>
struct std::formatter<Version> {
    constexpr auto parse(std::format_parse_context& context)
    {
        return context.begin();
    }

    // libc++ tests formatters against a minimal context
    // (basic_format_context<char*, char>), so format() must be context-generic
    // rather than taking std::format_context by name.
    template <class Context>
    auto format(const Version& version, Context& context) const
    {
        return std::format_to(context.out(), "{}.{}", version.major, version.minor);
    }
};

namespace {

void check_custom_and_runtime_formats()
{
    // User-defined formatters cannot go through the consteval format-string
    // validator with GCC 13: libc++ 18's __handle_replacement_field reaches its
    // "invalid argument index" path for handle arguments, and the no-exception
    // abort is not a constant expression. The runtime path below exercises the
    // same formatter, so the capability is still covered.
    Version version{1, 4};
    check_text(std::vformat("v{}", std::make_format_args(version)), "v1.4",
               "custom formatter");

    std::string buffer;
    std::format_to(std::back_inserter(buffer), "{}-{}", 3, "four");
    check_text(buffer, "3-four", "format_to back_inserter");

    char raw[32];
    auto end = std::format_to(raw, "[{}]", 17);
    check(std::string(raw, end) == "[17]", "format_to raw buffer");

    std::string format_string = std::format("{}{}", "runtime", "{}");
    int nine = 9;
    check_text(std::vformat(format_string, std::make_format_args(nine)), "runtime9",
               "vformat");

    check(std::formatted_size("{}", 12345) == 5, "formatted_size");

    std::vector<std::string> pieces;
    for (int i = 0; i < 3; ++i) {
        pieces.push_back(std::format("#{:02}", i));
    }
    check_text(pieces[0] + "," + pieces[2], "#00,#02", "format in a loop");
}

void check_print()
{
    // std::print writes straight to fd 1; the harness matches these markers in
    // the serial log, so a silent failure here is visible.
    std::print("print-check {}\n", 1);
    std::println("println-check {}", 2);
    std::println("{}", std::string("println-string"));
    std::print("{}", "");
    check(true, "print calls returned");
}

void check_expected()
{
    std::expected<int, std::string> good = 21;
    check(good.has_value() && *good == 21, "expected value");
    check(good.value_or(-1) == 21, "expected value_or");

    std::expected<int, std::string> bad = std::unexpected("denied");
    check(!bad.has_value(), "expected error");
    check(bad.error() == "denied", "expected error payload");

    auto doubled = good.transform([](int value) { return value * 2; });
    check(doubled.value() == 42, "expected transform");

    auto chained = good.and_then([](int value) -> std::expected<int, std::string> {
        if (value < 0) {
            return std::unexpected("negative");
        }
        return value + 1;
    });
    check(chained.value() == 22, "expected and_then");
}

void check_syncstream()
{
    std::ostringstream sink;
    {
        std::osyncstream sync(sink);
        sync << "atomic" << ' ' << "line" << '\n';
    }
    check(sink.str() == "atomic line\n", "osyncstream to string stream");

    auto writer = [&sink](char letter) {
        std::osyncstream sync(sink);
        sync << letter;
        for (int i = 0; i < 100; ++i) {
            std::osyncstream inner(sink);
            inner << "";
        }
    };
    std::thread first(writer, 'a');
    std::thread second(writer, 'b');
    first.join();
    second.join();
    check(sink.str().size() == std::string("atomic line\n").size() + 2,
          "osyncstream from two threads");
}

void check_mdspan()
{
    int storage[6] = {0, 1, 2, 3, 4, 5};
    std::mdspan<int, std::extents<size_t, 2, 3>> matrix(storage);
    check(matrix.extent(0) == 2 && matrix.extent(1) == 3, "mdspan extents");
    check(matrix[1, 2] == 5, "mdspan indexing");
    check(matrix.rank() == 2, "mdspan rank");
    check(matrix.size() == 6, "mdspan size");
}

} // namespace

int main()
{
    std::println("libc++ format/print probe on OCTAGRAM");
    check_integers();
    check_strings();
    check_floats();
    check_custom_and_runtime_formats();
    check_print();
    check_expected();
    check_syncstream();
    check_mdspan();

    if (failures != 0) {
        std::println("LIBCXX-FORMAT-FAIL {}", failures);
        return 1;
    }
    std::println("LIBCXX-FORMAT-PASS");
    return 0;
}
