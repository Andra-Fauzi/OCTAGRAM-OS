#ifndef OCTAGRAM_SPAWN_H
#define OCTAGRAM_SPAWN_H

#include <sys/types.h>
#include <signal.h>

#define POSIX_SPAWN_RESETIDS 0x0001
#define POSIX_SPAWN_SETPGROUP 0x0002
#define POSIX_SPAWN_SETSIGDEF 0x0004
#define POSIX_SPAWN_SETSIGMASK 0x0008
/* Atribut scheduling belum didukung kernel; posix_spawn mengembalikan ENOTSUP. */
#define POSIX_SPAWN_SETSCHEDPARAM 0x0040
#define POSIX_SPAWN_SETSCHEDULER 0x0080

/* File actions are applied in the child after fork and before execve. */
#define OCTAGRAM_SPAWN_MAX_FILE_ACTIONS 8
#define OCTAGRAM_SPAWN_MAX_PATH 128

enum octagram_spawn_action_kind {
    OCTAGRAM_SPAWN_ACTION_CLOSE = 1,
    OCTAGRAM_SPAWN_ACTION_DUP2 = 2,
    OCTAGRAM_SPAWN_ACTION_OPEN = 3,
};

struct octagram_spawn_file_action {
    int kind;
    int fd;
    int target_fd;
    int flags;
    mode_t mode;
    char path[OCTAGRAM_SPAWN_MAX_PATH];
};

typedef struct octagram_posix_spawn_file_actions posix_spawn_file_actions_t;
typedef struct octagram_posix_spawn_attributes posix_spawnattr_t;

struct octagram_posix_spawn_file_actions {
    int count;
    struct octagram_spawn_file_action actions[OCTAGRAM_SPAWN_MAX_FILE_ACTIONS];
};

struct octagram_posix_spawn_attributes {
    unsigned short flags;
    pid_t pgroup;
    sigset_t sigmask;
    sigset_t sigdefault;
};

int posix_spawn_file_actions_init(posix_spawn_file_actions_t *actions);
int posix_spawn_file_actions_destroy(posix_spawn_file_actions_t *actions);
int posix_spawn_file_actions_addclose(posix_spawn_file_actions_t *actions,
                                      int fd);
int posix_spawn_file_actions_adddup2(posix_spawn_file_actions_t *actions,
                                     int source_fd, int target_fd);
int posix_spawn_file_actions_addopen(posix_spawn_file_actions_t *actions,
                                     int fd, const char *path, int flags,
                                     mode_t mode);
int posix_spawnattr_init(posix_spawnattr_t *attributes);
int posix_spawnattr_destroy(posix_spawnattr_t *attributes);
int posix_spawnattr_setflags(posix_spawnattr_t *attributes, short flags);
int posix_spawnattr_getflags(const posix_spawnattr_t *attributes,
                             short *flags);
int posix_spawnattr_setpgroup(posix_spawnattr_t *attributes, pid_t group);
int posix_spawnattr_getpgroup(const posix_spawnattr_t *attributes,
                              pid_t *group);
int posix_spawnattr_setsigdefault(posix_spawnattr_t *attributes,
                                  const sigset_t *mask);
int posix_spawnattr_getsigdefault(const posix_spawnattr_t *attributes,
                                  sigset_t *mask);
int posix_spawnattr_setsigmask(posix_spawnattr_t *attributes,
                               const sigset_t *mask);
int posix_spawnattr_getsigmask(const posix_spawnattr_t *attributes,
                               sigset_t *mask);

int posix_spawn(pid_t *restrict pid, const char *restrict path,
                const posix_spawn_file_actions_t *restrict file_actions,
                const posix_spawnattr_t *restrict attributes,
                char *const argv[restrict], char *const environment[restrict]);
int posix_spawnp(pid_t *restrict pid, const char *restrict file,
                 const posix_spawn_file_actions_t *restrict file_actions,
                 const posix_spawnattr_t *restrict attributes,
                 char *const argv[restrict], char *const environment[restrict]);

#endif
