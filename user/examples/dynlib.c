#include "octagram.h"

int shared_library_counter = 7;

/* Dua versi dari satu nama simbol; referensi tanpa versi memakai versi default. */
int shared_version_v1(void)
{
    return 1;
}

int shared_version_v2(void)
{
    return 2;
}

__asm__(".symver shared_version_v1,shared_version@EXAMPLE_1.0");
__asm__(".symver shared_version_v2,shared_version@@EXAMPLE_2.0");

int shared_library_counter_value(void)
{
    return shared_library_counter;
}

__attribute__((destructor)) static void shared_library_destructor(void)
{
    static const char message[] = "shared library finalized\n";
    (void)oct_write(OCT_STDOUT, message, sizeof(message) - 1);
}

int shared_library_message(void)
{
    static const char message[] = "dynamic shared library loaded\n";
    return (int)oct_write(OCT_STDOUT, message, sizeof(message) - 1);
}
