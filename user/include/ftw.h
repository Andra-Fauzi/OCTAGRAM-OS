#ifndef OCTAGRAM_FTW_H
#define OCTAGRAM_FTW_H

#include <sys/stat.h>

#ifdef __cplusplus
extern "C" {
#endif

#define FTW_F 0
#define FTW_D 1
#define FTW_DNR 2
#define FTW_NS 3
#define FTW_SL 4
#define FTW_DP 6
#define FTW_SLN 7

#define FTW_PHYS 0x01
#define FTW_MOUNT 0x02
#define FTW_DEPTH 0x04
#define FTW_CHDIR 0x08

struct FTW {
    int base;
    int level;
};

/* Walks `path` depth first. `descriptors` is accepted for compatibility: the
 * walk holds at most one directory open at a time. FTW_CHDIR is ignored for
 * the same reason, and the callback always sees the full path. */
int nftw(const char *path,
         int (*function)(const char *, const struct stat *, int, struct FTW *),
         int descriptors, int flags);
int ftw(const char *path,
        int (*function)(const char *, const struct stat *, int), int descriptors);

#ifdef __cplusplus
}
#endif
#endif
