#ifndef OCTAGRAM_FEATURES_H
#define OCTAGRAM_FEATURES_H

/* OCTAGRAM is freestanding: it is not glibc, so this header deliberately
 * defines none of the glibc feature macros. libc++ includes <features.h> while
 * configuring itself, and providing this file lets the whole C++ build run with
 * -nostdinc so no host header can leak into the guest image. */

/* Some translation units mix OCTAGRAM headers with host glibc headers -- the
 * vendored TinyCC does exactly that in tccrun.c. glibc's <signal.h> and its
 * relatives expect its own <features.h> to have pulled in <sys/cdefs.h>, so when
 * a host header is reachable, chain to it. Guest builds use -nostdinc, so there
 * is nothing to chain and this block disappears. */
#if defined(__has_include_next)
#if __has_include_next(<features.h>)
#include_next <features.h>
#endif
#endif

#endif
