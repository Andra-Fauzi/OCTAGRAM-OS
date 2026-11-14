#ifndef OCTAGRAM_UNISTD_H
#define OCTAGRAM_UNISTD_H

#include <stddef.h>
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
int dup2(int source_fd, int target_fd);
int dup(int fd);
int ftruncate(int fd, off_t length);

#endif
