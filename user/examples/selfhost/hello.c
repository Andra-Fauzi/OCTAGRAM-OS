#include "octagram.h"

const char *selfhost_message(void);

int main(void)
{
    int64_t socket = oct_udp_socket();
    if (socket < 0 || oct_udp_bind((int)socket, 0) < 49152) {
        return 1;
    }
    if (oct_close((int)socket) < 0) {
        return 2;
    }
    if (oct_yield() < 0) {
        return 3;
    }
    const char *message = selfhost_message();
    return oct_write(OCT_STDOUT, message, 23) < 0;
}
