#ifndef OCTAGRAM_ENDIAN_H
#define OCTAGRAM_ENDIAN_H

/* Byte-order macros and host/network conversion helpers.
 *
 * OCTAGRAM runs on x86-64 only, so the target is always little-endian; the
 * values still follow the glibc/BSD names because portable code (LLVM's
 * llvm/ADT/bit.h, libc++ internals, various crates' C shims) tests them
 * instead of compiler builtins like __BYTE_ORDER__.
 *
 * The __* forms are the reserved names glibc exposes unconditionally, while
 * BYTE_ORDER/BIG_ENDIAN/LITTLE_ENDIAN are only defined when they are not
 * already provided by another header.
 */

#include <stdint.h>

#define __LITTLE_ENDIAN 1234
#define __BIG_ENDIAN 4321
#define __PDP_ENDIAN 3412
#define __BYTE_ORDER __LITTLE_ENDIAN

#ifndef LITTLE_ENDIAN
#define LITTLE_ENDIAN __LITTLE_ENDIAN
#endif
#ifndef BIG_ENDIAN
#define BIG_ENDIAN __BIG_ENDIAN
#endif
#ifndef PDP_ENDIAN
#define PDP_ENDIAN __PDP_ENDIAN
#endif
#ifndef BYTE_ORDER
#define BYTE_ORDER __BYTE_ORDER
#endif

#ifndef __bswap_16
#define __bswap_16(value) __builtin_bswap16((uint16_t)(value))
#endif
#ifndef __bswap_32
#define __bswap_32(value) __builtin_bswap32((uint32_t)(value))
#endif
#ifndef __bswap_64
#define __bswap_64(value) __builtin_bswap64((uint64_t)(value))
#endif

/* Little-endian is the native order, so these are the identity. */
#define htole16(value) ((uint16_t)(value))
#define htole32(value) ((uint32_t)(value))
#define htole64(value) ((uint64_t)(value))
#define le16toh(value) ((uint16_t)(value))
#define le32toh(value) ((uint32_t)(value))
#define le64toh(value) ((uint64_t)(value))

#define htobe16(value) __builtin_bswap16((uint16_t)(value))
#define htobe32(value) __builtin_bswap32((uint32_t)(value))
#define htobe64(value) __builtin_bswap64((uint64_t)(value))
#define be16toh(value) __builtin_bswap16((uint16_t)(value))
#define be32toh(value) __builtin_bswap32((uint32_t)(value))
#define be64toh(value) __builtin_bswap64((uint64_t)(value))

#endif
