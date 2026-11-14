#ifndef OCTAGRAM_SPAWN_H
#define OCTAGRAM_SPAWN_H

#include <sys/types.h>

typedef struct octagram_posix_spawn_file_actions posix_spawn_file_actions_t;
typedef struct octagram_posix_spawn_attributes posix_spawnattr_t;

int posix_spawn(pid_t *restrict pid, const char *restrict path,
                const posix_spawn_file_actions_t *restrict file_actions,
                const posix_spawnattr_t *restrict attributes,
                char *const argv[restrict], char *const environment[restrict]);
int posix_spawnp(pid_t *restrict pid, const char *restrict file,
                 const posix_spawn_file_actions_t *restrict file_actions,
                 const posix_spawnattr_t *restrict attributes,
                 char *const argv[restrict], char *const environment[restrict]);

#endif
