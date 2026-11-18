/* Uji urutan handler atexit/__cxa_atexit di atas runtime asli (host build). */
#include <stdlib.h>

static char order[8];
static int count;
static int first_dso;
static int second_dso;

static void plain(void)
{
    order[count++] = 'p';
}

static void argument(void *value)
{
    order[count++] = *(const char *)value;
}

int main(void)
{
    char first = 'a';
    char second = 'b';

    if (atexit(plain) != 0) {
        return 1;
    }
    if (__cxa_atexit(argument, &first, &first_dso) != 0) {
        return 2;
    }
    if (__cxa_atexit(argument, &second, &second_dso) != 0) {
        return 3;
    }

    /* Filter DSO: hanya handler milik DSO pertama yang berjalan. */
    __cxa_finalize(&first_dso);
    if (count != 1 || order[0] != 'a') {
        return 4;
    }

    /* Sisanya berjalan terbalik: handler DSO kedua, lalu atexit. */
    __cxa_finalize(NULL);
    if (count != 3 || order[1] != 'b' || order[2] != 'p') {
        return 5;
    }

    /* Finalize kedua tidak menjalankan ulang handler yang sudah selesai. */
    __cxa_finalize(NULL);
    if (count != 3) {
        return 6;
    }
    return 0;
}
