#ifndef OCTAGRAM_MACHINE_ENDIAN_H
#define OCTAGRAM_MACHINE_ENDIAN_H

/* BSD-style location for the byte-order macros.
 *
 * Portable code that is not compiled with __linux__ defined (OCTAGRAM defines
 * no host OS macro) reaches <endian.h> through this path instead; LLVM's
 * llvm/ADT/bit.h does exactly that. The definitions live in <endian.h> so
 * there is only one source of truth. */

#include <endian.h>

#endif
