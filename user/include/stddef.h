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

#endif
