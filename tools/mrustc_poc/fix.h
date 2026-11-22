/* Bytewise compatibility shim for mrustc's unreachable AtomicU128 path.
 * This is deliberately NOT an atomic operation and must not be used by
 * executing code or concurrent threads. It avoids unsupported __int128 syntax
 * in TinyCC while allowing unrelated core code to be compiled for this PoC.
 */
#ifndef OCTAGRAM_POC_FIX_H
#define OCTAGRAM_POC_FIX_H

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
