#ifndef OCTAGRAM_DIRENT_H
#define OCTAGRAM_DIRENT_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stdint.h>
#include <sys/types.h>

#define DT_UNKNOWN 0
#define DT_FIFO 1
#define DT_CHR 2
#define DT_DIR 4
#define DT_BLK 6
#define DT_REG 8
#define DT_LNK 10
#define DT_SOCK 12

typedef struct octagram_DIR DIR;

struct dirent {
    uint64_t d_ino;
    off_t d_off;
    uint16_t d_reclen;
    unsigned char d_type;
    char d_name[256];
};

DIR *opendir(const char *path);
DIR *fdopendir(int fd);
struct dirent *readdir(DIR *directory);
int closedir(DIR *directory);
int dirfd(DIR *directory);

/* Collect a directory into a NULL-terminated, caller-freed array of copies.
 * The comparison takes slots, matching alphasort/versionsort. */
int scandir(const char *path, struct dirent ***namelist,
            int (*filter)(const struct dirent *),
            int (*compar)(const struct dirent **, const struct dirent **));
int scandirat(int dirfd, const char *path, struct dirent ***namelist,
              int (*filter)(const struct dirent *),
              int (*compar)(const struct dirent **, const struct dirent **));
int alphasort(const struct dirent **left, const struct dirent **right);
int versionsort(const struct dirent **left, const struct dirent **right);


#ifdef __cplusplus
}
#endif
#endif
