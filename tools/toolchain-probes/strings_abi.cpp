#include <strings.h>

int main(int argc, char **argv)
{
    const char *left = argc > 1 ? argv[1] : "OpenSSL";
    const char *right = argc > 2 ? argv[2] : "openssl";

    /* Dynamic arguments keep the cross-link probe from constant-folding the
     * calls: both public POSIX symbols must resolve from OCTAGRAM libc.a. */
    return strcasecmp(left, right) + strncasecmp(left, right, 4);
}
