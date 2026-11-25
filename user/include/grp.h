#ifndef OCTAGRAM_GRP_H
#define OCTAGRAM_GRP_H

#include <stddef.h>
#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

struct group {
    char *gr_name;
    char *gr_passwd;
    gid_t gr_gid;
    char **gr_mem;
};

struct group *getgrnam(const char *name);
struct group *getgrgid(gid_t gid);
struct group *getgrent(void);
void setgrent(void);
void endgrent(void);

int getgrnam_r(const char *name, struct group *entry, char *buffer,
               size_t size, struct group **result);
int getgrgid_r(gid_t gid, struct group *entry, char *buffer, size_t size,
               struct group **result);


#ifdef __cplusplus
}
#endif
#endif
