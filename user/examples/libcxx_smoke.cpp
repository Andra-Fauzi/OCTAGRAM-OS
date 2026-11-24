// Smoke test for the vendored libc++ headers on OCTAGRAM.
//
// Only headers whose inline code needs nothing from libc++'s compiled library
// are used here: traits, limits, new, and the container/utility headers whose
// implementations are entirely in headers. <string>'s out-of-line functions
// (basic_string::__init, ::append) still need libc++'s sources, so it is not
// used yet. tools/check_libcxx_headers.py tracks which headers compile, and
// docs/rust-port-guide.md records what the rest still need.
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <new>
#include <optional>
#include <string_view>
#include <tuple>
#include <type_traits>
#include <utility>
#include <vector>

#include <stdio.h>

static int failures;

static void check(bool condition, const char* what)
{
    if (!condition) {
        ++failures;
        printf("FAIL %s\n", what);
    }
}

namespace {

template <typename Type, typename std::enable_if<std::is_integral<Type>::value, int>::type = 0>
constexpr Type twice(Type value)
{
    return static_cast<Type>(value + value);
}

template <typename Type, typename std::enable_if<!std::is_integral<Type>::value, int>::type = 0>
constexpr Type twice(Type value)
{
    return value + value;
}

} // namespace

int main()
{
    check(std::is_same<std::remove_cv_t<const int>, int>::value, "std::remove_cv_t");
    check(std::is_integral<unsigned short>::value && !std::is_integral<double>::value,
          "std::is_integral");
    check(twice(21) == 42 && twice(1.5) == 3.0, "enable_if overload resolution");

    check(std::numeric_limits<unsigned long long>::max() == ~0ull, "numeric_limits::max");
    check(std::numeric_limits<int>::is_signed && !std::numeric_limits<unsigned>::is_signed,
          "numeric_limits::is_signed");
    check(std::numeric_limits<double>::has_infinity, "numeric_limits<double>");

    std::byte storage[16];
    std::memset(storage, 0, sizeof storage);
    storage[0] = std::byte{0x2a};
    check(std::to_integer<int>(storage[0]) == 42, "std::byte and std::to_integer");

    check(sizeof(std::size_t) == 8 && sizeof(std::ptrdiff_t) == 8, "cstddef types");

    int* heap = new int(7);
    check(heap != nullptr && *heap == 7, "operator new");
    int* many = new int[4];
    many[3] = 9;
    check(many[3] == 9, "operator new[]");
    delete heap;
    delete[] many;

    std::array<int, 4> fixed{{4, 1, 3, 2}};
    check(std::min(fixed[0], fixed[1]) == 1 && std::max(fixed[2], fixed[3]) == 4,
          "std::array with std::min/std::max");

    std::vector<int> growable;
    for (int value = 0; value < 5; ++value) {
        growable.push_back(value * value);
    }
    check(growable.size() == 5 && growable[4] == 16, "std::vector push_back");
    check(growable.capacity() >= growable.size(), "std::vector capacity");
    int total = 0;
    for (const int value : growable) {
        total += value;
    }
    check(total == 30, "std::vector range-for");

    std::optional<int> present = 5;
    std::optional<int> missing;
    check(present.has_value() && *present == 5 && !missing.has_value(), "std::optional");

    std::pair<int, int> pair = std::make_pair(1, 2);
    check(pair.first == 1 && pair.second == 2, "std::pair");
    const auto swapped = std::make_pair(2, 1);
    check(swapped.first > swapped.second, "std::make_pair");

    std::tuple<int, double, const char*> record{3, 2.5, "ok"};
    check(std::get<0>(record) == 3 && std::get<1>(record) == 2.5, "std::tuple and std::get");

    check(std::min(3, 5) == 3 && std::max(3, 5) == 5, "std::min and std::max");

    const std::string_view view("octagram", 7);
    check(view.size() == 7 && view.substr(2, 3) == "tag", "std::string_view");

    if (failures != 0) {
        printf("libcxx smoke: FAIL (%d)\n", failures);
        return 1;
    }
    printf("LIBCXX-SMOKE-PASS\n");
    return 0;
}
