/* First freestanding C++ program for OCTAGRAM (M4.3.1).
 *
 * Compiled with the host g++ in freestanding mode (-fno-exceptions,
 * -fno-rtti, -nostdinc++) and linked against OCTAGRAM's own crt0 and C
 * runtime, so it depends on no libstdc++/libc++ and no dynamic loader.
 * Run as `/bin/cpp-smoke` from the OCTAGRAM shell.
 */
#include <pthread.h>
#include <stdio.h>
#include <wctype.h>

namespace octagram {

template <typename T>
constexpr T larger(T left, T right)
{
    return left > right ? left : right;
}

struct Shape {
    const char *label;
    int width;
    int height;

    virtual int area() const = 0;
    virtual const char *name() const = 0;
};

struct Rectangle final : Shape {
    Rectangle(const char *label, int width, int height)
    {
        this->label = label;
        this->width = width;
        this->height = height;
    }

    int area() const override
    {
        return width * height;
    }

    const char *name() const override
    {
        return label;
    }
};

struct Triangle final : Shape {
    Triangle(const char *label, int base, int height)
    {
        this->label = label;
        this->width = base;
        this->height = height;
    }

    int area() const override
    {
        return width * height / 2;
    }

    const char *name() const override
    {
        return label;
    }
};

/* Static-storage object with a dynamic constructor: it can only be correct if
 * crt0 runs .init_array before main(). */
static int constructor_count;

struct ConstructorProbe {
    int payload;

    ConstructorProbe() : payload(41)
    {
        ++constructor_count;
    }
};

static ConstructorProbe global_probe;

} // namespace octagram

static int failures;

static void check(int condition, const char *what)
{
    if (condition) {
        printf("ok   %s\n", what);
    } else {
        printf("FAIL %s\n", what);
        ++failures;
    }
}

static int local_static_value()
{
    static int value = 1234;
    return value;
}

/* thread_local objects with a non-trivial destructor are registered through
 * __cxa_thread_atexit and unwound when the owning thread exits. */
static int thread_local_dtors;

struct ThreadLocalProbe {
    int value;

    ThreadLocalProbe() : value(7) {}
    ~ThreadLocalProbe()
    {
        ++thread_local_dtors;
    }
};

static thread_local ThreadLocalProbe thread_local_probe;

static void *thread_local_entry(void *)
{
    thread_local_probe.value = 11;
    return nullptr;
}

int main(int argc, char **argv)
{
    (void)argv;

    using octagram::Rectangle;
    using octagram::Triangle;

    /* Runs before anything else in main: proves .init_array was executed. */
    check(octagram::constructor_count == 1 && octagram::global_probe.payload == 41,
          "static constructor ran before main via .init_array");
    check(local_static_value() == 1234, "function-local static storage");

    /* libc++ builds std::call_once and guarded statics on pthread_once. */
    static pthread_once_t once = PTHREAD_ONCE_INIT;
    static int once_runs;
    pthread_once(&once, []() { ++once_runs; });
    pthread_once(&once, []() { ++once_runs; });
    check(once_runs == 1, "pthread_once runs the initializer exactly once");

    /* Wide-character classification backs libc++ locale/ctype facets. */
    check(iswalpha(L'A') && !iswalpha(L'1') && towlower(L'Q') == L'q',
          "wide-character classification and case mapping");

    pthread_attr_t thread_attributes;
    pthread_attr_init(&thread_attributes);
    pthread_t thread_local_thread = 0;
    const bool thread_started =
        pthread_create(&thread_local_thread, &thread_attributes, thread_local_entry, nullptr) == 0;
    if (thread_started) {
        pthread_join(thread_local_thread, nullptr);
    }
    check(thread_started && thread_local_dtors == 1,
          "thread_local destructor ran when its thread exited");

    /* 128-bit division is a libcall (__udivmodti4) on x86-64, so this also
     * proves the compiler-rt helpers are linked in. */
    const unsigned __int128 wide_left =
        (static_cast<unsigned __int128>(0x123456789abcdefull) << 64) | 0xfedcba9876543210ull;
    const unsigned __int128 wide_right = 0x100000001ull;
    const unsigned __int128 wide_quotient = wide_left / wide_right;
    const unsigned __int128 wide_remainder = wide_left % wide_right;
    check(wide_quotient * wide_right + wide_remainder == wide_left &&
              wide_remainder < wide_right && wide_quotient != 0,
          "128-bit integer division and remainder");

    /* Conversions use the __floatuntidf/__fixunsdfti libcalls; 2^100 is exact
     * in a double, so the round trip must be bit-exact. */
    const unsigned __int128 wide_power = static_cast<unsigned __int128>(1) << 100;
    const double wide_power_as_double = static_cast<double>(wide_power);
    check(wide_power_as_double == 0x1p100 &&
              static_cast<unsigned __int128>(wide_power_as_double) == wide_power &&
              static_cast<double>(-static_cast<__int128>(wide_power)) == -0x1p100,
          "128-bit and floating point conversion round trip");

    Rectangle *heap_rectangle = new Rectangle("heap", 3, 5);
    check(heap_rectangle->area() == 15, "operator new and a heap-allocated object");
    delete heap_rectangle;

    int *numbers = new int[3];
    numbers[0] = 4;
    numbers[1] = 5;
    numbers[2] = 6;
    check(numbers[0] + numbers[1] + numbers[2] == 15, "operator new[] and operator delete[]");
    delete[] numbers;

    Rectangle rectangle("rectangle", 6, 7);
    Triangle triangle("triangle", 6, 7);
    octagram::Shape *shapes[2] = {&rectangle, &triangle};

    check(argc >= 1, "argc is provided by crt0");
    check(octagram::larger(3, 9) == 9, "constexpr function template");
    check(rectangle.area() == 42, "virtual dispatch through a base pointer");
    check(shapes[0]->area() == 42 && shapes[1]->area() == 21, "vtable entries resolve per type");

    const char *names[2] = {shapes[0]->name(), shapes[1]->name()};
    check(names[0][0] == 'r' && names[1][0] == 't', "virtual const char* return value");

    /* References, enum class, and range-based for are pure language features
     * that need no C++ runtime support. */
    enum class Scale { single = 1, double_size = 2 };
    const Scale scale = Scale::double_size;
    int total = 0;
    for (const octagram::Shape *shape : shapes) {
        total += shape->area() * static_cast<int>(scale);
    }
    check(total == 126, "range-based for over pointers and static_cast enum");

    if (failures != 0) {
        printf("C++ freestanding runtime: FAIL (%d)\n", failures);
        return 1;
    }
    printf("C++ freestanding runtime: PASS\n");
    printf("CPP-FREESTANDING-PASS\n");
    return 0;
}
