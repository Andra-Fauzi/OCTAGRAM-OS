#include "octagram.h"

int main(void)
{
    for (uint64_t y = 0; y < 32; y++) {
        for (uint64_t x = 0; x < 32; x++) {
            uint32_t color = (uint32_t)(x * 8) << 16
                           | (uint32_t)(y * 8) << 8
                           | 0x40;
            if (oct_putpixel(32 + x, 48 + y, color) < 0) {
                return 1;
            }
        }
    }
    return 0;
}
