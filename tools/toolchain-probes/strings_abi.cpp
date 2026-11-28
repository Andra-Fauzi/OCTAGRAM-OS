#include <strings.h>

int main()
{
    return strcasecmp("OpenSSL", "openssl") != 0
        || strncasecmp("OCTAGRAM", "octa", 4) != 0
        || strncasecmp("OCTAGRAM", "OCTA", 4) != 0;
}
