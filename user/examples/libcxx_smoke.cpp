// Smoke test for the vendored libc++ headers on OCTAGRAM.
//
// Only the headers that already build against OCTAGRAM's C library are used
// here: <type_traits>, <limits>, <new>, <cstddef>, <cstdint> and <cstring>.
// tools/check_libcxx_headers.py tracks which of the rest compile, and
// docs/rust-port-guide.md records what the remaining ones still need.
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <new>
#include <type_traits>

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

    if (failures != 0) {
        printf("libcxx smoke: FAIL (%d)\n", failures);
        return 1;
    }
    printf("LIBCXX-SMOKE-PASS\n");
    return 0;
}
