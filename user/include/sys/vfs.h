/*
 * <sys/vfs.h> for OCTAGRAM.
 *
 * The target is not Linux (rustc's target_os is "octagram"), but OCTAGRAM's
 * userspace ABI deliberately mirrors the Linux header surface, and LLVM is
 * compiled with the host compiler, which predefines __linux__. LLVM's
 * lib/Support/Unix/Path.inc therefore takes its Linux branch and asks for
 * statfs()/struct statfs from this header.
 *
 * There is no filesystem-capacity syscall yet, so statfs()/fstatfs() report
 * ENOSYS exactly like statvfs()/fstatvfs() in <sys/statvfs.h>; callers such as
 * std::filesystem::space() surface "not implemented" instead of a fabricated
 * free-space figure. The struct layout follows Linux x86-64 so code compiled
 * against the Linux branch stays ABI-consistent with the rest of the port.
 */
#ifndef OCTAGRAM_SYS_VFS_H
#define OCTAGRAM_SYS_VFS_H

#include <stddef.h>
#include <sys/statvfs.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    int __val[2];
} fsid_t;

typedef long int __fsword_t;

struct statfs {
    __fsword_t f_type;    /* type of filesystem */
    __fsword_t f_bsize;   /* optimal transfer block size */
    fsblkcnt_t f_blocks;  /* total data blocks in filesystem */
    fsblkcnt_t f_bfree;   /* free blocks in filesystem */
    fsblkcnt_t f_bavail;  /* free blocks available to unprivileged user */
    fsfilcnt_t f_files;   /* total file nodes in filesystem */
    fsfilcnt_t f_ffree;   /* free file nodes in filesystem */
    fsid_t f_fsid;        /* filesystem ID */
    __fsword_t f_namelen; /* maximum length of filenames */
    __fsword_t f_frsize;  /* fragment size */
    __fsword_t f_flags;   /* mount flags */
    __fsword_t f_spare[4];
};

int statfs(const char *path, struct statfs *buffer);
int fstatfs(int descriptor, struct statfs *buffer);

#ifdef __cplusplus
}
#endif

#endif /* OCTAGRAM_SYS_VFS_H */
