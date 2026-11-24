#ifndef OCTAGRAM_STDDEF_H
#define OCTAGRAM_STDDEF_H

typedef __SIZE_TYPE__ size_t;
typedef __PTRDIFF_TYPE__ ptrdiff_t;
// wchar_t is a builtin type in C++, so it must not be typedef'd again there.
#ifndef __cplusplus
typedef __WCHAR_TYPE__ wchar_t;
#endif

#ifndef NULL
#ifdef __cplusplus
#define NULL nullptr
#else
#define NULL ((void *)0)
#endif
#endif

#define offsetof(type, member) __builtin_offsetof(type, member)

/* C11 requires a type whose alignment is at least as strict as any scalar type;
 * long double gives the 16-byte alignment the x86-64 ABI uses for it. */
typedef struct {
    long long __octagram_long_long;
    long double __octagram_long_double;
} max_align_t;

#endif
