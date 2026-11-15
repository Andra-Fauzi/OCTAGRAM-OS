#include "octagram.h"

int main(void)
{
    static const char message[] = "Built inside OCTAGRAM!\n";
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
    return oct_write(OCT_STDOUT, message, sizeof(message) - 1) < 0;
}
