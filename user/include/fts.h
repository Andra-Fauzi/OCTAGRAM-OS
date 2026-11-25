#ifndef OCTAGRAM_FTS_H
#define OCTAGRAM_FTS_H

#include <sys/stat.h>
#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _ftsent {
    struct _ftsent *fts_cycle;  /* ancestor that closes a cycle, or NULL */
    struct _ftsent *fts_parent; /* directory containing this entry */
    struct _ftsent *fts_link;   /* next entry from fts_children */
    long fts_number;            /* scratch field for the caller */
    void *fts_pointer;          /* scratch field for the caller */
    char *fts_accpath;          /* access path (same as fts_path here) */
    char *fts_path;             /* path from the fts_open root */
    int fts_errno;              /* errno for FTS_NS/FTS_DNR/FTS_ERR */
    int fts_symfd;              /* unused: always -1 (no chdir mode) */
    unsigned short fts_pathlen;
    unsigned short fts_namelen;
    ino_t fts_ino;
    dev_t fts_dev;
    nlink_t fts_nlink;
    short fts_level;            /* depth, 0 for the roots */
    unsigned short fts_info;
    unsigned short fts_flags;
    unsigned short fts_instr;
    struct stat *fts_statp;     /* NULL when FTS_NOSTAT skipped it */
    char fts_name[1];           /* basename, over-allocated */
} FTSENT;

typedef struct _fts FTS;

/* fts_open options. */
#define FTS_COMFOLLOW 0x0001
#define FTS_LOGICAL 0x0002
#define FTS_NOCHDIR 0x0004
#define FTS_NOSTAT 0x0008
#define FTS_PHYSICAL 0x0010
#define FTS_SEEDOT 0x0020
#define FTS_XDEV 0x0040
#define FTS_WHITEOUT 0x0080

/* fts_set instructions. */
#define FTS_SKIP 1
#define FTS_CHDIRED 2
#define FTS_DOT 3

/* fts_info values. */
#define FTS_D 1
#define FTS_DC 2
#define FTS_DEFAULT 3
#define FTS_DNR 4
/* 5 is FTS_DOT as an fts_set() instruction, declared below. */
#define FTS_DP 6
#define FTS_ERR 7
#define FTS_F 8
#define FTS_INIT 9
#define FTS_NS 10
#define FTS_NSOK 11
#define FTS_SL 12
#define FTS_SLNONE 13
#define FTS_W 14

FTS *fts_open(char *const *path_argv, int options,
              int (*compar)(const FTSENT **, const FTSENT **));
FTSENT *fts_read(FTS *stream);
FTSENT *fts_children(FTS *stream, int instructions);
int fts_set(FTS *stream, FTSENT *entry, int instructions);
int fts_close(FTS *stream);

#ifdef __cplusplus
}
#endif
#endif
