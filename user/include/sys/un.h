#ifndef OCTAGRAM_SYS_UN_H
#define OCTAGRAM_SYS_UN_H

/* POSIX defines struct sockaddr_un here; OCTAGRAM's <sys/socket.h> already
 * carries the definition (AF_UNIX socket pairs are the kernel's socketpair
 * path), so this header is the standard include path for portable code such as
 * LLVM's raw_socket_stream.cpp rather than a second definition. */

#include <sys/socket.h>

#endif
