#ifndef OCTAGRAM_SETJMP_H
#define OCTAGRAM_SETJMP_H

typedef struct {
    unsigned long registers[8];
} octagram_jmp_buf[1];

typedef octagram_jmp_buf jmp_buf;

int _setjmp(jmp_buf environment) __attribute__((returns_twice));
__attribute__((noreturn)) void longjmp(jmp_buf environment, int value);

#define setjmp(environment) _setjmp(environment)
#define sigsetjmp(environment, save_mask) ((void)(save_mask), _setjmp(environment))
#define siglongjmp longjmp

#endif
