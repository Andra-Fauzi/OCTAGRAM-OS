#ifndef OCTAGRAM_DIRENT_H
#define OCTAGRAM_DIRENT_H

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
struct dirent *readdir(DIR *directory);
int closedir(DIR *directory);

#endif
