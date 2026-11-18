#include "octagram.h"

extern int shared_version(void);
extern int shared_version_old(void);
extern int shared_library_counter_value(void);

__asm__(".symver shared_version_old,shared_version@EXAMPLE_1.0");

int main(void)
{
    if (shared_version() != 2) {
        return 1;
    }
    if (shared_version_old() != 1) {
        return 2;
    }
    if (shared_library_counter_value() != 7) {
        return 3;
    }

    static const char message[] = "symbol versioning passed\n";
    if (oct_write(OCT_STDOUT, message, sizeof(message) - 1) < 0) {
        return 4;
    }
    return 0;
}
