#include "octagram.h"

int main(void)
{
    static const char message[] = "Built inside OCTAGRAM!\n";
    return oct_write(OCT_STDOUT, message, sizeof(message) - 1) < 0;
}
