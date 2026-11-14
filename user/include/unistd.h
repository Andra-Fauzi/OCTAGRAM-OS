#ifndef OCTAGRAM_UNISTD_H
#define OCTAGRAM_UNISTD_H

#include <stddef.h>
#include <sys/types.h>

#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2

int close(int fd);
int execvp(const char *file, char *const argv[]);
ssize_t read(int fd, void *buffer, size_t count);
ssize_t write(int fd, const void *buffer, size_t count);
off_t lseek(int fd, off_t offset, int whence);
int unlink(const char *path);
int chdir(const char *path);
char *getcwd(char *buffer, size_t size);
pid_t getpid(void);
int isatty(int fd);
int pipe(int descriptors[2]);
int dup2(int source_fd, int target_fd);
int ftruncate(int fd, off_t length);

#endif
