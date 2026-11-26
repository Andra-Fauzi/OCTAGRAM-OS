#ifndef OCTAGRAM_SETJMP_H
#define OCTAGRAM_SETJMP_H

/* The buffer type needs a struct tag: an unnamed struct makes the function
 * declarations "declared using unnamed type ... never defined" for C++/C23
 * front ends, and LLVM includes <setjmp.h> from C++ (CrashRecoveryContext).
 * C linkage is required for the same reason: C++ callers must reach the
 * _setjmp/longjmp symbols defined in user/setjmp.S, not mangled names. */

#ifdef __cplusplus
extern "C" {
#endif

struct __octagram_jmp_buf {
    unsigned long registers[8];
};

typedef struct __octagram_jmp_buf octagram_jmp_buf[1];

typedef octagram_jmp_buf jmp_buf;

int _setjmp(jmp_buf environment) __attribute__((returns_twice));
__attribute__((noreturn)) void longjmp(jmp_buf environment, int value);

#ifdef __cplusplus
}
#endif

#define setjmp(environment) _setjmp(environment)
#define sigsetjmp(environment, save_mask) ((void)(save_mask), _setjmp(environment))
#define siglongjmp longjmp

#endif
