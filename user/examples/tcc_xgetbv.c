#include <stdint.h>

/*
 * Compile-and-inspect probe for XGETBV. The test's main() scans the function
 * bytes for 0F 01 D0 but never calls this function: executing it is conditional
 * on CPUID.OSXSAVE and CR4.OSXSAVE/XCR0 state.
 */
unsigned long long octagram_read_xcr(unsigned int index)
{
    unsigned int low, high;

    __asm__ __volatile__("xgetbv"
                         : "=a"(low), "=d"(high)
                         : "c"(index));
    return ((unsigned long long)high << 32) | low;
}

int main(void)
{
    unsigned long long (*function)(unsigned int) = octagram_read_xcr;
    const unsigned char *code =
        (const unsigned char *)(uintptr_t)function;
    unsigned int index;

    for (index = 0; index + 2 < 64; ++index) {
        if (code[index] == 0x0f && code[index + 1] == 0x01 &&
            code[index + 2] == 0xd0)
            return 0;
    }
    return 33;
}
