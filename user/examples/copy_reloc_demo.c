#include "octagram.h"

/* Data dari shared library yang diakses executable non-PIC: ld menghasilkan
   relokasi R_X86_64_COPY di executable. */
extern int shared_library_counter;
extern int shared_library_counter_value(void);

/* Simbol weak yang tidak disediakan library harus bernilai NULL. */
extern int shared_library_optional(void) __attribute__((weak));

static int write_text(const char *text, unsigned long length)
{
    return (int)oct_write(OCT_STDOUT, text, length);
}

int main(void)
{
    if (shared_library_counter != 7) {
        return 1;
    }
    if (shared_library_counter_value() != 7) {
        return 2;
    }
    shared_library_counter = 11;
    if (shared_library_counter != 11) {
        return 3;
    }
    if (shared_library_counter_value() != 11) {
        return 4;
    }
    if (shared_library_optional != 0) {
        return 5;
    }

    static const char message[] = "copy relocation passed\n";
    if (write_text(message, sizeof(message) - 1) < 0) {
        return 6;
    }
    return 0;
}
