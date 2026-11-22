/*
 * Compile-only probe for the XGETBV assembler instruction.
 * Do not execute this function unless CPUID reports OSXSAVE support and the
 * operating system has enabled the required XCR0 state components.
 */
unsigned long long octagram_read_xcr(unsigned int index)
{
    unsigned int low, high;

    __asm__ __volatile__("xgetbv"
                         : "=a"(low), "=d"(high)
                         : "c"(index));
    return ((unsigned long long)high << 32) | low;
}
