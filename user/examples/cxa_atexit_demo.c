#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>

static char order[8];
static int order_length;

static void record(char marker)
{
    if (order_length < (int)sizeof(order)) {
        order[order_length++] = marker;
    }
}

static void plain_handler(void)
{
    record('p');
}

static void argument_handler(void *argument)
{
    record(*(const char *)argument);
}

int main(void)
{
    char first = 'a';
    char second = 'b';
    static int first_dso;
    static int second_dso;

    if (atexit(plain_handler) != 0) {
        return 1;
    }
    if (__cxa_atexit(argument_handler, &first, &first_dso) != 0) {
        return 2;
    }
    if (__cxa_atexit(argument_handler, &second, &second_dso) != 0) {
        return 3;
    }

    /* Hanya handler milik DSO pertama yang berjalan. */
    __cxa_finalize(&first_dso);
    if (order_length != 1 || order[0] != 'a') {
        return 4;
    }

    /* Sisanya berjalan terbalik: handler kedua lebih dulu, lalu atexit. */
    __cxa_finalize(NULL);
    if (order_length != 3 || order[1] != 'b' || order[2] != 'p') {
        return 5;
    }

    printf("cxa_atexit ordering passed\n");
    return 0;
}
