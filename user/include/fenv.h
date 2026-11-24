#ifndef OCTAGRAM_FENV_H
#define OCTAGRAM_FENV_H

#ifdef __cplusplus
extern "C" {
#endif

/* x87 floating-point environment.
 *
 * OCTAGRAM masks SSE exceptions and never switches MXCSR, so the environment
 * here is the x87 control word plus the x87 status word. One deviation is worth
 * stating up front: the x87 cannot set individual sticky status bits, only
 * clear the whole status word (`fnclex`), so `feclearexcept` clears every
 * exception flag rather than just the requested ones. */
typedef struct {
    unsigned short control;
    unsigned short status;
} fenv_t;

typedef unsigned short fexcept_t;

#define FE_INVALID 0x01
#define FE_DENORMAL 0x02
#define FE_DIVBYZERO 0x04
#define FE_OVERFLOW 0x08
#define FE_UNDERFLOW 0x10
#define FE_INEXACT 0x20
#define FE_ALL_EXCEPT 0x3f

#define FE_TONEAREST 0x000
#define FE_DOWNWARD 0x400
#define FE_UPWARD 0x800
#define FE_TOWARDZERO 0xc00
#define FE_ROUND_MASK 0xc00

#define FE_DFL_ENV ((const fenv_t *)-1)

int feclearexcept(int excepts);
int fegetexceptflag(fexcept_t *flag, int excepts);
int feraiseexcept(int excepts);
int fesetexceptflag(const fexcept_t *flag, int excepts);
int fetestexcept(int excepts);
int fegetround(void);
int fesetround(int rounding);
int fegetenv(fenv_t *environment);
int feholdexcept(fenv_t *environment);
int fesetenv(const fenv_t *environment);
int feupdateenv(const fenv_t *environment);

#ifdef __cplusplus
}
#endif

#endif
