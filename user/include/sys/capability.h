#ifndef OCTAGRAM_SYS_CAPABILITY_H
#define OCTAGRAM_SYS_CAPABILITY_H

#include <stdint.h>
#include <sys/types.h>

/* Metadata kemampuan proses OCTAGRAM. Bit-bit ini belum ditegakkan di jalur
 * akses; kernel hanya menyimpannya dan membagikannya ke semua thread satu pid. */
#define CAP_VFS_WRITE (1ULL << 0)
#define CAP_NETWORK (1ULL << 1)
#define CAP_SYS_ADMIN (1ULL << 2)
#define CAP_SCHED_NICE (1ULL << 3)
#define CAP_ALL                                                                \
    (CAP_VFS_WRITE | CAP_NETWORK | CAP_SYS_ADMIN | CAP_SCHED_NICE)

/* pid 0 berarti proses pemanggil. capset membutuhkan CAP_SYS_ADMIN. */
int capget(pid_t pid, uint64_t *mask);
int capset(pid_t pid, uint64_t mask);

#endif
