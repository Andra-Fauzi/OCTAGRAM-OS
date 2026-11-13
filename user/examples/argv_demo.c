#include "octagram.h"

#include <string.h>

int main(int argc, char **argv)
{
    for (int index = 0; index < argc; ++index) {
        oct_write(OCT_STDOUT, argv[index], strlen(argv[index]));
        oct_write(OCT_STDOUT, "\n", 1);
    }
    return argc;
}
