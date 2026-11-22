/* Compatibility helpers that remain necessary for this limited PoC. */
#ifndef OCTAGRAM_POC_FIX_H
#define OCTAGRAM_POC_FIX_H

/* TinyCC now implements GCC's type-generic add/sub/mul overflow builtins.
   AtomicU128 is still unreachable in this PoC and is rewritten by patch_core.py. */
static int poc_cxchg128(unsigned __int128* obj, unsigned __int128* expected,
                        unsigned __int128 desired) {
    if (*obj == *expected) {
        *obj = desired;
        return 1;
    }
    *expected = *obj;
    return 0;
}
#endif
