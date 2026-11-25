// Fourth libc++ guest probe: filesystem, random, regex, and chrono.
//
// These are the remaining standard components a native Cargo/rustc host needs
// for path handling, temporary names, diagnostics, and timing. std::printf is
// used for markers so a partial failure names its stage.

#include <chrono>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <random>
#include <regex>
#include <string>

namespace fs = std::filesystem;
namespace chrono = std::chrono;

int main() {
    std::error_code error;
    const fs::path directory = "/fs-probe";
    fs::create_directories(directory, error);
    if (error || !fs::is_directory(directory, error)) {
        std::printf("LIBCXX-FILESYSTEM-FAIL: create_directories (%s)\n", error.message().c_str());
        return 1;
    }

    const fs::path file = directory / "data.txt";
    {
        std::ofstream output(file);
        if (!output.is_open()) {
            std::printf("LIBCXX-FILESYSTEM-FAIL: open for write\n");
            return 1;
        }
        output << "hello";
    }
    if (!fs::exists(file, error) || !fs::is_regular_file(file, error)) {
        std::printf("LIBCXX-FILESYSTEM-FAIL: exists/is_regular_file\n");
        return 1;
    }
    if (fs::file_size(file, error) != 5) {
        std::printf("LIBCXX-FILESYSTEM-FAIL: file_size\n");
        return 1;
    }
    if (file.filename() != "data.txt" || file.parent_path() != directory) {
        std::printf("LIBCXX-FILESYSTEM-FAIL: path decomposition\n");
        return 1;
    }
    if (fs::path("a/b/../c").lexically_normal() != fs::path("a/c")) {
        std::printf("LIBCXX-FILESYSTEM-FAIL: lexically_normal\n");
        return 1;
    }

    int entries = 0;
    for (const fs::directory_entry& entry : fs::directory_iterator(directory, error)) {
        (void)entry;
        ++entries;
    }
    if (error || entries != 1) {
        std::printf("LIBCXX-FILESYSTEM-FAIL: directory_iterator (%d entries)\n", entries);
        return 1;
    }
    if (!fs::remove_all(directory, error) || error) {
        std::printf("LIBCXX-FILESYSTEM-FAIL: remove_all\n");
        return 1;
    }
    std::printf("LIBCXX-FILESYSTEM-PASS\n");

    // std::random: the engine and distribution must be deterministic per seed.
    std::mt19937 engine(12345);
    std::mt19937 replay(12345);
    std::uniform_int_distribution<int> distribution(1, 6);
    for (int roll = 0; roll < 8; ++roll) {
        int first = distribution(engine);
        int second = distribution(replay);
        if (first != second || first < 1 || first > 6) {
            std::printf("LIBCXX-RANDOM-FAIL: mt19937 determinism\n");
            return 1;
        }
    }
    std::printf("LIBCXX-RANDOM-PASS\n");

    // std::regex: enough of the engine for Cargo-style version matching.
    std::regex pattern("([a-z]+)-([0-9]+)");
    std::smatch match;
    const std::string subject = "octagram-42";
    if (!std::regex_match(subject, match, pattern) || match.size() != 3 ||
        match[1] != "octagram" || match[2] != "42") {
        std::printf("LIBCXX-REGEX-FAIL: match groups\n");
        return 1;
    }
    if (!std::regex_search(std::string("v1.2.3"), std::regex("[0-9]+\\.[0-9]+"))) {
        std::printf("LIBCXX-REGEX-FAIL: search\n");
        return 1;
    }
    std::printf("LIBCXX-REGEX-PASS\n");

    // std::chrono: system and steady clocks plus a measured sleep-free span.
    const auto system_start = chrono::system_clock::now();
    const auto steady_start = chrono::steady_clock::now();
    volatile unsigned long spins = 0;
    for (unsigned long index = 0; index < 100000; ++index) {
        spins += index;
    }
    const auto steady_end = chrono::steady_clock::now();
    if (steady_end < steady_start) {
        std::printf("LIBCXX-CHRONO-FAIL: steady clock went backwards\n");
        return 1;
    }
    if (chrono::system_clock::to_time_t(system_start) <= 0) {
        std::printf("LIBCXX-CHRONO-FAIL: system clock epoch\n");
        return 1;
    }
    std::printf("LIBCXX-CHRONO-PASS\n");

    std::printf("LIBCXX-FS-SUITE-PASS\n");
    return 0;
}
