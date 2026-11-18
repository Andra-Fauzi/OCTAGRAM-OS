#include "octagram.h"
#include <stdio.h>
#include <stdlib.h>

int shared_library_message(void);

static void report_atexit(void)
{
    static const char message[] = "atexit handler ran\n";
    (void)oct_write(OCT_STDOUT, message, sizeof(message) - 1);
}

int main(void)
{
    if (atexit(report_atexit) != 0) {
        return 1;
    }
    if (printf("shared libc loaded\n") < 0) {
        return 2;
    }
    return shared_library_message() < 0 ? 3 : 0;
}
