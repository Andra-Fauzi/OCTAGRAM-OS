#include "octagram.h"

#include <stdlib.h>
#include <string.h>

int main(void)
{
    static const char message[] = "Small runtime is linked.\n";
    char *copy = malloc(sizeof(message));
    if (copy == NULL) {
        return 1;
    }

    memcpy(copy, message, sizeof(message));
    if (memcmp(copy, message, sizeof(message)) != 0) {
        return 2;
    }
    return oct_write(OCT_STDOUT, copy, strlen(copy)) < 0;
}
