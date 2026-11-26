#ifndef OCTAGRAM_CPUID_H
#define OCTAGRAM_CPUID_H

/*
 * GCC-compatible <cpuid.h>.
 *
 * This header ships with the host GCC installation, which is excluded from the
 * OCTAGRAM sysroot (-nostdinc), but portable x86 code includes it directly:
 * LLVM's lib/TargetParser/Host.cpp calls __get_cpuid/__get_cpuid_count to
 * detect the host CPU. The CPUID instruction itself is unprivileged, so these
 * helpers work in the guest unchanged.
 *
 * The interface mirrors GCC's: __cpuid, __cpuid_count, __get_cpuid,
 * __get_cpuid_count, __get_cpuid_max and the common bit_* feature masks.
 */

#if !defined(__i386__) && !defined(__x86_64__)
#error "cpuid.h is only available on x86 targets"
#endif

static __inline void __cpuid(unsigned int level, unsigned int *eax,
                             unsigned int *ebx, unsigned int *ecx,
                             unsigned int *edx)
{
    __asm__ __volatile__("cpuid"
                         : "=a"(*eax), "=b"(*ebx), "=c"(*ecx), "=d"(*edx)
                         : "a"(level));
}

static __inline void __cpuid_count(unsigned int level, unsigned int count,
                                   unsigned int *eax, unsigned int *ebx,
                                   unsigned int *ecx, unsigned int *edx)
{
    __asm__ __volatile__("cpuid"
                         : "=a"(*eax), "=b"(*ebx), "=c"(*ecx), "=d"(*edx)
                         : "a"(level), "c"(count));
}

static __inline unsigned int __get_cpuid_max(unsigned int extended,
                                             unsigned int *signature)
{
    unsigned int eax = 0;
    unsigned int ebx = 0;
    unsigned int ecx = 0;
    unsigned int edx = 0;
    if ((extended & 0x80000000U) != 0) {
        __cpuid(0x80000000U, &eax, &ebx, &ecx, &edx);
    } else {
        __cpuid(0, &eax, &ebx, &ecx, &edx);
    }
    if (signature != 0) {
        *signature = ebx;
    }
    return eax;
}

static __inline int __get_cpuid(unsigned int level, unsigned int *eax,
                                unsigned int *ebx, unsigned int *ecx,
                                unsigned int *edx)
{
    if (__get_cpuid_max(level & 0x80000000U, 0) < level) {
        return 0;
    }
    __cpuid(level, eax, ebx, ecx, edx);
    return 1;
}

static __inline int __get_cpuid_count(unsigned int level, unsigned int count,
                                      unsigned int *eax, unsigned int *ebx,
                                      unsigned int *ecx, unsigned int *edx)
{
    if (__get_cpuid_max(level & 0x80000000U, 0) < level) {
        return 0;
    }
    __cpuid_count(level, count, eax, ebx, ecx, edx);
    return 1;
}

/* ECX bits of leaf 1. */
#define bit_SSE3 (1U << 0)
#define bit_PCLMUL (1U << 1)
#define bit_SSSE3 (1U << 9)
#define bit_FMA (1U << 12)
#define bit_CMPXCHG16B (1U << 13)
#define bit_SSE4_1 (1U << 19)
#define bit_SSE4_2 (1U << 20)
#define bit_MOVBE (1U << 22)
#define bit_POPCNT (1U << 23)
#define bit_AES (1U << 25)
#define bit_XSAVE (1U << 26)
#define bit_OSXSAVE (1U << 27)
#define bit_AVX (1U << 28)
#define bit_F16C (1U << 29)
#define bit_RDRND (1U << 30)

/* EDX bits of leaf 1. */
#define bit_FPU (1U << 0)
#define bit_MMX (1U << 23)
#define bit_SSE (1U << 25)
#define bit_SSE2 (1U << 26)

/* EBX bits of leaf 7, subleaf 0. */
#define bit_FSGSBASE (1U << 0)
#define bit_BMI (1U << 3)
#define bit_AVX2 (1U << 5)
#define bit_BMI2 (1U << 8)
#define bit_AVX512F (1U << 16)
#define bit_AVX512DQ (1U << 17)
#define bit_AVX512IFMA (1U << 21)
#define bit_AVX512PF (1U << 26)
#define bit_AVX512ER (1U << 27)
#define bit_AVX512CD (1U << 28)
#define bit_AVX512BW (1U << 30)
#define bit_AVX512VL (1U << 31)

#endif /* OCTAGRAM_CPUID_H */
