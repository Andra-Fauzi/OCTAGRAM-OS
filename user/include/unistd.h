#ifndef OCTAGRAM_UNISTD_H
#define OCTAGRAM_UNISTD_H

#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>

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

#endif
