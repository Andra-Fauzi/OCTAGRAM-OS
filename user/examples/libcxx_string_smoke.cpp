// First smoke test for the compiled libc++ library (libc++.a), not just its
// headers. The previous probe (libcxx_smoke.cpp) only used header-only
// facilities; std::string/std::sort/std::mutex need the explicit
// instantiations and runtime symbols that live in third_party/libcxx/src.
//
// Prints one marker per facility so a guest run pinpoints the first failing
// step. The final marker is LIBCXX-LIBRARY-PASS, then the process exits 0.

#include <algorithm>
#include <charconv>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <mutex>
#include <string>
#include <vector>

int main() {
    std::string text = "octagram";
    text += "-rust";
    text.append(1, '!');
    if (text != "octagram-rust!") {
        std::printf("LIBCXX-STRING-FAIL: append\n");
        return 1;
    }
    if (text.find("gram") != 5) {
        std::printf("LIBCXX-STRING-FAIL: find\n");
        return 1;
    }
    std::string slice = text.substr(9, 4);
    if (slice != "rust") {
        std::printf("LIBCXX-STRING-FAIL: substr\n");
        return 1;
    }
    // Long enough to force the heap-backed representation.
    std::string long_text(120, 'x');
    long_text.insert(0, "prefix-");
    if (long_text.size() != 127 || long_text.compare(0, 7, "prefix-") != 0) {
        std::printf("LIBCXX-STRING-FAIL: heap\n");
        return 1;
    }
    std::printf("LIBCXX-STRING-PASS\n");

    // <charconv> integers, the path Cargo uses to read version numbers and
    // lockfile metadata without locale or iostream support.
    char buffer[64];
    auto int_written = std::to_chars(buffer, buffer + sizeof buffer, 4242);
    if (int_written.ec != std::errc() || std::string(buffer, int_written.ptr) != "4242") {
        std::printf("LIBCXX-CHARCONV-FAIL: integer to_chars\n");
        return 1;
    }
    int parsed_integer = 0;
    auto int_read = std::from_chars(buffer, int_written.ptr, parsed_integer);
    if (int_read.ec != std::errc() || parsed_integer != 4242) {
        std::printf("LIBCXX-CHARCONV-FAIL: integer from_chars\n");
        return 1;
    }
    unsigned long long wide_value = 0;
    const char* wide_digits = "18446744073709551615";
    auto wide_read = std::from_chars(wide_digits, wide_digits + std::strlen(wide_digits), wide_value);
    if (wide_read.ec != std::errc() || wide_value != 18446744073709551615ULL) {
        std::printf("LIBCXX-CHARCONV-FAIL: 64-bit from_chars\n");
        return 1;
    }

    // <charconv> floating point needs the FLT_*/DBL_* limits in
    // user/include/float.h and libc++'s ryu implementation.
    auto float_written = std::to_chars(buffer, buffer + sizeof buffer, 1.5);
    if (float_written.ec != std::errc() || std::string(buffer, float_written.ptr) != "1.5") {
        std::printf("LIBCXX-CHARCONV-FAIL: floating to_chars\n");
        return 1;
    }
    // libc++ 18 has no floating-point std::from_chars (that overload is
    // MSVC-only), so only to_chars is exercised here; the double overload is
    // deliberately deleted upstream.
    std::printf("LIBCXX-CHARCONV-PASS\n");

    // <string> numeric helpers built on the same machinery.
    std::string decimal = std::to_string(-1234);
    if (decimal != "-1234") {
        std::printf("LIBCXX-STRING-FAIL: to_string\n");
        return 1;
    }
    if (std::stoi("  +77rest") != 77) {
        std::printf("LIBCXX-STRING-FAIL: stoi\n");
        return 1;
    }
    std::printf("LIBCXX-TOSTRING-PASS\n");

    std::vector<int> numbers;
    for (int value : {5, 3, 9, 1, 7}) {
        numbers.push_back(value);
    }
    std::sort(numbers.begin(), numbers.end());
    if (numbers.size() != 5 || numbers.front() != 1 || numbers.back() != 9) {
        std::printf("LIBCXX-VECTOR-FAIL\n");
        return 1;
    }
    std::printf("LIBCXX-VECTOR-PASS\n");

    std::mutex guard;
    guard.lock();
    int protected_value = 41;
    protected_value += 1;
    guard.unlock();
    if (protected_value != 42) {
        std::printf("LIBCXX-MUTEX-FAIL\n");
        return 1;
    }
    std::printf("LIBCXX-MUTEX-PASS\n");

    // std::to_wstring goes through swprintf, which user/wchar.c now provides.
    std::wstring wide = std::to_wstring(65535L);
    if (wide != L"65535") {
        std::printf("LIBCXX-STRING-FAIL: to_wstring\n");
        return 1;
    }
    wchar_t formatted[32];
    if (swprintf(formatted, 32, L"%ld/%04d/%s", 123456L, 7, "ok") < 0 ||
        wcslen(formatted) != 14 || formatted[0] != L'1') {
        std::printf("LIBCXX-WIDE-FAIL: swprintf\n");
        return 1;
    }
    std::printf("LIBCXX-WIDE-PASS\n");

    std::printf("LIBCXX-LIBRARY-PASS\n");
    return 0;
}
