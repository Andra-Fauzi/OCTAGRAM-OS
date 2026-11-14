#include "octagram.h"
#include <stdio.h>

int shared_library_message(void);

int main(void)
{
    if (printf("shared libc loaded\n") < 0) {
        return 1;
    }
    return shared_library_message() < 0;
}
