#include "octagram.h"

int shared_library_message(void)
{
    static const char message[] = "dynamic shared library loaded\n";
    return (int)oct_write(OCT_STDOUT, message, sizeof(message) - 1);
}
