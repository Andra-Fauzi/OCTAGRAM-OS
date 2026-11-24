#ifndef OCTAGRAM_FEATURES_H
#define OCTAGRAM_FEATURES_H

/* OCTAGRAM is freestanding: it is not glibc, so this header deliberately
 * defines none of the glibc feature macros. libc++ includes <features.h> while
 * configuring itself, and providing this file lets the whole C++ build run with
 * -nostdinc so no host header can leak into the guest image. */

#endif
