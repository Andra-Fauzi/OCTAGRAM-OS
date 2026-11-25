// First smoke test for the compiled libc++ library (libc++.a), not just its
// headers. The previous probe (libcxx_smoke.cpp) only used header-only
// facilities; std::string/std::sort/std::mutex need the explicit
// instantiations and runtime symbols that live in third_party/libcxx/src.
//
// Prints one marker per facility so a guest run pinpoints the first failing
// step. The final marker is LIBCXX-LIBRARY-PASS, then the process exits 0.

#include <algorithm>
#include <cstdio>
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

    std::printf("LIBCXX-LIBRARY-PASS\n");
    return 0;
}
