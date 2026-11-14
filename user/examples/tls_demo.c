#include "octagram.h"
#include <stdio.h>

static __thread volatile unsigned long initialized_value = 41;
static __thread volatile unsigned long zero_initialized_value;

int main(void)
{
    if (oct_get_thread_pointer() == 0) {
        return 20;
    }
    if (initialized_value != 41) {
        return 1;
    }
    if (zero_initialized_value != 0) {
        return 2;
    }
    initialized_value += 1;
    zero_initialized_value = initialized_value;
    if (initialized_value != 42 || zero_initialized_value != 42) {
        return 3;
    }

    puts("static TLS initialized");
    return 0;
}
