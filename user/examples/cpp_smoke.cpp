/* First freestanding C++ program for OCTAGRAM (M4.3.1).
 *
 * Compiled with the host g++ in freestanding mode (-fno-exceptions,
 * -fno-rtti, -nostdinc++) and linked against OCTAGRAM's own crt0 and C
 * runtime, so it depends on no libstdc++/libc++ and no dynamic loader.
 * Run as `/bin/cpp-smoke` from the OCTAGRAM shell.
 */
#include <stdio.h>

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

int main(int argc, char **argv)
{
    (void)argv;

    using octagram::Rectangle;
    using octagram::Triangle;

    /* Runs before anything else in main: proves .init_array was executed. */
    check(octagram::constructor_count == 1 && octagram::global_probe.payload == 41,
          "static constructor ran before main via .init_array");
    check(local_static_value() == 1234, "function-local static storage");

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
