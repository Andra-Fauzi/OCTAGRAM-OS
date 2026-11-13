#include "octagram.h"

int main(void)
{
    static const char path[] = "/hello.txt";
    static const char message[] = "Created by a user ELF.\n";
    char buffer[64];

    int64_t fd = oct_open(path, OCT_O_RDWR | OCT_O_CREAT | OCT_O_TRUNC);
    if (fd < 0) {
        return 1;
    }
    if (oct_write((int)fd, message, sizeof(message) - 1) < 0
        || oct_lseek((int)fd, 0, OCT_SEEK_SET) < 0) {
        oct_close((int)fd);
        return 2;
    }

    int64_t count = oct_read((int)fd, buffer, sizeof(buffer));
    oct_close((int)fd);
    if (count < 0) {
        return 3;
    }
    return oct_write(OCT_STDOUT, buffer, (size_t)count) < 0;
}
