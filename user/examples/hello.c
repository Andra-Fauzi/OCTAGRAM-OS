#include "octagram.h"

int main(void)
{
    static const char message[] = "Hello from OCTAGRAM user mode!\n";
    return oct_write(OCT_STDOUT, message, sizeof(message) - 1) < 0;
}
