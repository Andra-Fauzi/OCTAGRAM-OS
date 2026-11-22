/* Bytewise compatibility shim for mrustc's unreachable AtomicU128 path.
 * This is deliberately NOT an atomic operation and must not be used by
 * executing code or concurrent threads. It avoids unsupported __int128 syntax
 * in TinyCC while allowing unrelated core code to be compiled for this PoC.
 */
#ifndef OCTAGRAM_POC_FIX_H
#define OCTAGRAM_POC_FIX_H

/* TinyCC has no __builtin_bswapNN, and implicit declarations would leave the
 * calls unresolved at link time. Plain byte shuffles are exact and cheap.
 * Host compilers provide these as real builtins, and GCC rejects a redeclaration
 * whose signature differs from its builtin, so define them for TinyCC only. */
#ifdef __TINYC__
static unsigned short __builtin_bswap16(unsigned short value)
{
    return (unsigned short)((value >> 8) | (value << 8));
}

static unsigned int __builtin_bswap32(unsigned int value)
{
    return ((value & 0x000000ffu) << 24) |
           ((value & 0x0000ff00u) << 8) |
           ((value & 0x00ff0000u) >> 8) |
           ((value & 0xff000000u) >> 24);
}

static unsigned long long __builtin_bswap64(unsigned long long value)
{
    return ((unsigned long long)__builtin_bswap32((unsigned int)value) << 32) |
           (unsigned long long)__builtin_bswap32((unsigned int)(value >> 32));
}
#endif /* __TINYC__ */

static int poc_cxchg128(void *object, void *expected, const void *desired)
{
    volatile unsigned char *target = (volatile unsigned char *)object;
    unsigned char *expected_bytes = (unsigned char *)expected;
    const unsigned char *desired_bytes = (const unsigned char *)desired;
    unsigned int index;

    for (index = 0; index < 16; ++index) {
        if (target[index] != expected_bytes[index]) {
            unsigned int copy;
            for (copy = 0; copy < 16; ++copy)
                expected_bytes[copy] = target[copy];
            return 0;
        }
    }

    for (index = 0; index < 16; ++index)
        target[index] = desired_bytes[index];
    return 1;
}
#endif
