#ifndef OCTAGRAM_UNISTD_H
#define OCTAGRAM_UNISTD_H

/* Advertise the POSIX interfaces declared by this header. */
#ifndef _POSIX_VERSION
#define _POSIX_VERSION 200809L
#endif

#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>
#include <time.h>

extern char **environ;

#define STDIN_FILENO 0
#define STDOUT_FILENO 1
#define STDERR_FILENO 2
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#define F_OK 0
#define X_OK 1
#define W_OK 2
#define R_OK 4

#define _SC_ARG_MAX 0
#define _SC_CHILD_MAX 1
#define _SC_CLK_TCK 2
#define _SC_NGROUPS_MAX 3
#define _SC_OPEN_MAX 4
#define _SC_HOST_NAME_MAX 5
#define _SC_GETPW_R_SIZE_MAX 6
#define _SC_PAGESIZE 30
#define _SC_PAGE_SIZE 30
#define _SC_NPROCESSORS_CONF 83
#define _SC_NPROCESSORS_ONLN 84

int close(int fd);
int access(const char *path, int mode);
int execve(const char *path, char *const argv[], char *const envp[]);
int execvp(const char *file, char *const argv[]);
pid_t fork(void);
int rmdir(const char *path);
int symlink(const char *target, const char *link_path);
int symlinkat(const char *target, int dirfd, const char *link_path);
ssize_t readlink(const char *path, char *buffer, size_t size);
ssize_t readlinkat(int dirfd, const char *path, char *buffer, size_t size);
ssize_t read(int fd, void *buffer, size_t count);
ssize_t write(int fd, const void *buffer, size_t count);
off_t lseek(int fd, off_t offset, int whence);
int unlink(const char *path);
int unlinkat(int dirfd, const char *path, int flags);
int chdir(const char *path);
char *getcwd(char *buffer, size_t size);
pid_t getpid(void);
int isatty(int fd);
int pipe(int descriptors[2]);
int pipe2(int descriptors[2], int flags);
int dup2(int source_fd, int target_fd);
int dup(int fd);
int ftruncate(int fd, off_t length);
uid_t getuid(void);
gid_t getgid(void);
uid_t geteuid(void);
gid_t getegid(void);
int brk(void *address);
void *sbrk(intptr_t increment);
unsigned int sleep(unsigned int seconds);
int usleep(useconds_t microseconds);
long sysconf(int name);
pid_t getppid(void);
int fsync(int fd);
int fdatasync(int fd);
long syscall(long number, ...);
ssize_t pread(int fd, void *buffer, size_t count, off_t offset);
ssize_t pwrite(int fd, const void *buffer, size_t count, off_t offset);
int truncate(const char *path, off_t length);
int dup3(int source_fd, int target_fd, int flags);
int close_range(unsigned int first, unsigned int last, int flags);
int chown(const char *path, uid_t owner, gid_t group);
int lchown(const char *path, uid_t owner, gid_t group);
int fchown(int fd, uid_t owner, gid_t group);
int setuid(uid_t uid);
int seteuid(uid_t uid);
int setgid(gid_t gid);
int setegid(gid_t gid);
int getgroups(int size, gid_t *groups);
pid_t getpgid(pid_t pid);
pid_t getpgrp(void);
int setpgid(pid_t pid, pid_t pgid);
pid_t setsid(void);
pid_t getsid(pid_t pid);
int setreuid(uid_t real, uid_t effective);
int setregid(gid_t real, gid_t effective);
int setresuid(uid_t real, uid_t effective, uid_t saved);
int setresgid(gid_t real, gid_t effective, gid_t saved);
int getresuid(uid_t *real, uid_t *effective, uid_t *saved);
int getresgid(gid_t *real, gid_t *effective, gid_t *saved);
int setgroups(size_t size, const gid_t *groups);
int nice(int increment);
int pause(void);
unsigned int alarm(unsigned int seconds);
char *ctermid(char *name);
char *ttyname(int fd);
int ttyname_r(int fd, char *buffer, size_t size);
long fpathconf(int fd, int name);
long pathconf(const char *path, int name);
int faccessat(int dirfd, const char *path, int mode, int flags);
int fchmodat(int dirfd, const char *path, mode_t mode, int flags);
int fchownat(int dirfd, const char *path, uid_t owner, gid_t group, int flags);
int utimensat(int dirfd, const char *path, const struct timespec *times, int flags);
int futimens(int fd, const struct timespec times[2]);
__attribute__((noreturn)) int _exit(int status);
void sync(void);
int gethostname(char *name, size_t size);
size_t confstr(int name, char *buffer, size_t size);
char *getlogin(void);
int getloadavg(double load[], int count);
int lockf(int fd, int operation, off_t length);
int mkfifo(const char *path, mode_t mode);
int mkfifoat(int dirfd, const char *path, mode_t mode);
int mknod(const char *path, mode_t mode, dev_t device);
int mknodat(int dirfd, const char *path, mode_t mode, dev_t device);
int lchmod(const char *path, mode_t mode);
int execv(const char *path, char *const argv[]);
int sethostname(const char *name, size_t size);

#endif
