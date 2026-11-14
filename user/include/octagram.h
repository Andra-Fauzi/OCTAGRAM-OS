#ifndef OCTAGRAM_H
#define OCTAGRAM_H

#include <stddef.h>
#include <stdint.h>

extern char **environ;

char *getenv(const char *name);

#define OCT_STDIN  0
#define OCT_STDOUT 1
#define OCT_STDERR 2

#define OCT_O_RDONLY 0
#define OCT_O_WRONLY 1
#define OCT_O_RDWR   2
#define OCT_O_CREAT  (1u << 8)
#define OCT_O_TRUNC  (1u << 9)
#define OCT_O_APPEND (1u << 10)
#define OCT_AT_FDCWD (-100)
#define OCT_AT_SYMLINK_NOFOLLOW (1u << 8)
#define OCT_AT_REMOVEDIR (1u << 9)

#define OCT_SEEK_SET 0
#define OCT_SEEK_CUR 1
#define OCT_SEEK_END 2

#define OCT_PROT_READ  (1u << 0)
#define OCT_PROT_WRITE (1u << 1)
#define OCT_PROT_EXEC  (1u << 2)

#define OCT_PIPE_CAPACITY 4096
#define OCT_MAX_SYSCALL_IO 4096
#define OCT_WAIT_ANY UINT64_MAX
#define OCT_WNOHANG 1u

struct oct_stat {
    uint64_t size;
    uint32_t mode;
    uint32_t reserved;
};

static inline int64_t oct_syscall0(uint64_t number)
{
    int64_t result = (int64_t)number;
    __asm__ volatile ("int $0x80"
                      : "+a"(result)
                      :
                      : "rcx", "r11", "memory", "cc");
    return result;
}

static inline int64_t oct_syscall1(uint64_t number, uint64_t arg1)
{
    int64_t result = (int64_t)number;
    __asm__ volatile ("int $0x80"
                      : "+a"(result)
                      : "D"(arg1)
                      : "rcx", "r11", "memory", "cc");
    return result;
}

static inline int64_t oct_syscall2(uint64_t number, uint64_t arg1, uint64_t arg2)
{
    int64_t result = (int64_t)number;
    __asm__ volatile ("int $0x80"
                      : "+a"(result)
                      : "D"(arg1), "S"(arg2)
                      : "rcx", "r11", "memory", "cc");
    return result;
}

static inline int64_t oct_syscall3(uint64_t number, uint64_t arg1,
                                   uint64_t arg2, uint64_t arg3)
{
    int64_t result = (int64_t)number;
    __asm__ volatile ("int $0x80"
                      : "+a"(result)
                      : "D"(arg1), "S"(arg2), "d"(arg3)
                      : "rcx", "r11", "memory", "cc");
    return result;
}

static inline int64_t oct_syscall4(uint64_t number, uint64_t arg1,
                                   uint64_t arg2, uint64_t arg3, uint64_t arg4)
{
    int64_t result = (int64_t)number;
    register uint64_t fourth_argument __asm__("r10") = arg4;
    __asm__ volatile ("int $0x80"
                      : "+a"(result)
                      : "D"(arg1), "S"(arg2), "d"(arg3), "r"(fourth_argument)
                      : "rcx", "r11", "memory", "cc");
    return result;
}

static inline int64_t oct_syscall6(uint64_t number, uint64_t arg1,
                                   uint64_t arg2, uint64_t arg3,
                                   uint64_t arg4, uint64_t arg5,
                                   uint64_t arg6)
{
    int64_t result = (int64_t)number;
    register uint64_t fourth_argument __asm__("r10") = arg4;
    register uint64_t fifth_argument __asm__("r8") = arg5;
    register uint64_t sixth_argument __asm__("r9") = arg6;
    __asm__ volatile ("int $0x80"
                      : "+a"(result)
                      : "D"(arg1), "S"(arg2), "d"(arg3),
                        "r"(fourth_argument), "r"(fifth_argument),
                        "r"(sixth_argument)
                      : "rcx", "r11", "memory", "cc");
    return result;
}

static inline int64_t oct_exit(int status)
{
    return oct_syscall1(0, (uint64_t)(int64_t)status);
}

static inline int64_t oct_write(int fd, const void *buffer, size_t length)
{
    return oct_syscall3(1, (uint64_t)fd, (uint64_t)(uintptr_t)buffer, length);
}

static inline int64_t oct_read(int fd, void *buffer, size_t length)
{
    return oct_syscall3(2, (uint64_t)fd, (uint64_t)(uintptr_t)buffer, length);
}

static inline int64_t oct_open_mode(const char *path, uint64_t flags, uint32_t mode)
{
    return oct_syscall3(3, (uint64_t)(uintptr_t)path, flags, mode);
}

static inline int64_t oct_open(const char *path, uint64_t flags)
{
    return oct_open_mode(path, flags, 0644);
}

static inline int64_t oct_openat(int dirfd, const char *path, uint64_t flags,
                                 uint32_t mode)
{
    return oct_syscall4(40, (uint64_t)(int64_t)dirfd,
                        (uint64_t)(uintptr_t)path, flags, mode);
}

static inline int64_t oct_close(int fd)
{
    return oct_syscall1(4, (uint64_t)fd);
}

static inline int64_t oct_lseek(int fd, int64_t offset, int whence)
{
    return oct_syscall3(5, (uint64_t)fd, (uint64_t)offset, (uint64_t)whence);
}

static inline int64_t oct_stat_path(const char *path, struct oct_stat *result)
{
    return oct_syscall2(6, (uint64_t)(uintptr_t)path,
                        (uint64_t)(uintptr_t)result);
}

static inline int64_t oct_lstat_path(const char *path, struct oct_stat *result)
{
    return oct_syscall2(38, (uint64_t)(uintptr_t)path,
                        (uint64_t)(uintptr_t)result);
}

static inline int64_t oct_fchmod(int fd, uint32_t mode)
{
    return oct_syscall2(39, (uint64_t)fd, mode);
}

static inline int64_t oct_fstatat(int dirfd, const char *path,
                                  struct oct_stat *result, int flags)
{
    return oct_syscall4(41, (uint64_t)(int64_t)dirfd,
                        (uint64_t)(uintptr_t)path,
                        (uint64_t)(uintptr_t)result, (uint64_t)(unsigned int)flags);
}

static inline int64_t oct_mkdirat(int dirfd, const char *path, uint32_t mode)
{
    return oct_syscall3(42, (uint64_t)(int64_t)dirfd,
                        (uint64_t)(uintptr_t)path, mode);
}

static inline int64_t oct_unlinkat(int dirfd, const char *path, int flags)
{
    return oct_syscall3(43, (uint64_t)(int64_t)dirfd,
                        (uint64_t)(uintptr_t)path, (uint64_t)(unsigned int)flags);
}

static inline int64_t oct_renameat(int olddirfd, const char *oldpath,
                                   int newdirfd, const char *newpath)
{
    return oct_syscall4(44, (uint64_t)(int64_t)olddirfd,
                        (uint64_t)(uintptr_t)oldpath,
                        (uint64_t)(int64_t)newdirfd,
                        (uint64_t)(uintptr_t)newpath);
}

static inline int64_t oct_readlinkat(int dirfd, const char *path,
                                     char *buffer, size_t size)
{
    return oct_syscall4(45, (uint64_t)(int64_t)dirfd,
                        (uint64_t)(uintptr_t)path,
                        (uint64_t)(uintptr_t)buffer, size);
}

static inline int64_t oct_symlinkat(const char *target, int newdirfd,
                                    const char *link_path)
{
    return oct_syscall3(46, (uint64_t)(uintptr_t)target,
                        (uint64_t)(int64_t)newdirfd,
                        (uint64_t)(uintptr_t)link_path);
}

static inline int64_t oct_fstat(int fd, struct oct_stat *result)
{
    return oct_syscall2(29, (uint64_t)fd, (uint64_t)(uintptr_t)result);
}

static inline int64_t oct_access(const char *path, uint64_t mode)
{
    return oct_syscall2(30, (uint64_t)(uintptr_t)path, mode);
}

static inline int64_t oct_rename(const char *source, const char *target)
{
    return oct_syscall2(31, (uint64_t)(uintptr_t)source,
                        (uint64_t)(uintptr_t)target);
}

static inline int64_t oct_umask(uint32_t mask)
{
    return oct_syscall1(32, mask);
}

static inline int64_t oct_dup(int fd)
{
    return oct_syscall1(33, (uint64_t)fd);
}

static inline int64_t oct_fcntl(int fd, int command, uint64_t argument)
{
    return oct_syscall3(34, (uint64_t)fd, (uint64_t)command, argument);
}

static inline int64_t oct_rmdir(const char *path)
{
    return oct_syscall1(35, (uint64_t)(uintptr_t)path);
}

static inline int64_t oct_symlink(const char *target, const char *link_path)
{
    return oct_syscall2(36, (uint64_t)(uintptr_t)target,
                        (uint64_t)(uintptr_t)link_path);
}

static inline int64_t oct_readlink(const char *path, char *buffer, size_t size)
{
    return oct_syscall3(37, (uint64_t)(uintptr_t)path,
                        (uint64_t)(uintptr_t)buffer, size);
}

static inline int64_t oct_mkdir_mode(const char *path, uint32_t mode)
{
    return oct_syscall3(7, (uint64_t)(uintptr_t)path, mode, 0);
}

static inline int64_t oct_mkdir(const char *path)
{
    return oct_mkdir_mode(path, 0755);
}

static inline int64_t oct_unlink(const char *path)
{
    return oct_syscall1(8, (uint64_t)(uintptr_t)path);
}

static inline int64_t oct_ftruncate(int fd, size_t length)
{
    return oct_syscall2(9, (uint64_t)fd, length);
}

static inline int64_t oct_getpid(void)
{
    return oct_syscall0(11);
}

static inline int64_t oct_isatty(int fd)
{
    return oct_syscall1(12, (uint64_t)fd);
}

static inline int64_t oct_chdir(const char *path)
{
    return oct_syscall1(13, (uint64_t)(uintptr_t)path);
}

static inline int64_t oct_getcwd(char *buffer, size_t size)
{
    return oct_syscall2(14, (uint64_t)(uintptr_t)buffer, size);
}

static inline int64_t oct_chmod(const char *path, uint32_t mode)
{
    return oct_syscall2(25, (uint64_t)(uintptr_t)path, mode);
}

static inline int64_t oct_opendir(const char *path)
{
    return oct_syscall1(27, (uint64_t)(uintptr_t)path);
}

static inline int64_t oct_readdir(int fd, void *entry)
{
    return oct_syscall2(28, (uint64_t)fd, (uint64_t)(uintptr_t)entry);
}

static inline int64_t oct_execve(const char *path, char *const argv[],
                                 char *const environment[])
{
    return oct_syscall3(47, (uint64_t)(uintptr_t)path,
                        (uint64_t)(uintptr_t)argv,
                        (uint64_t)(uintptr_t)environment);
}

static inline int64_t oct_fork(void)
{
    return oct_syscall1(48, 0);
}

static inline int64_t oct_set_thread_pointer(void *thread_pointer)
{
    return oct_syscall1(59, (uint64_t)(uintptr_t)thread_pointer);
}

static inline int64_t oct_get_thread_pointer(void)
{
    return oct_syscall0(60);
}

static inline int64_t oct_thread_create(void *entry, void *argument,
                                        void *stack_top, void *thread_pointer,
                                        void *mapping_base, uint64_t mapping_length)
{
    return oct_syscall6(61, (uint64_t)(uintptr_t)entry,
                        (uint64_t)(uintptr_t)argument,
                        (uint64_t)(uintptr_t)stack_top,
                        (uint64_t)(uintptr_t)thread_pointer,
                        (uint64_t)(uintptr_t)mapping_base,
                        mapping_length);
}

static inline int64_t oct_thread_exit(void *result)
{
    return oct_syscall1(62, (uint64_t)(uintptr_t)result);
}

static inline int64_t oct_thread_join(uint64_t thread_id, void **result)
{
    return oct_syscall2(63, thread_id, (uint64_t)(uintptr_t)result);
}

static inline int64_t oct_thread_self(void)
{
    return oct_syscall0(64);
}

static inline int64_t oct_clock_gettime(int clock_id, void *time_value)
{
    return oct_syscall2(50, (uint64_t)(int64_t)clock_id,
                        (uint64_t)(uintptr_t)time_value);
}

static inline int64_t oct_waitpid(uint64_t pid, uint64_t *status)
{
    return oct_syscall3(20, pid, (uint64_t)(uintptr_t)status, 0);
}

static inline int64_t oct_waitpid_options(uint64_t pid, uint64_t *status,
                                          uint64_t options)
{
    return oct_syscall3(20, pid, (uint64_t)(uintptr_t)status, options);
}

static inline int64_t oct_pipe(int descriptors[2])
{
    return oct_syscall1(21, (uint64_t)(uintptr_t)descriptors);
}

static inline int64_t oct_dup2(int source_fd, int target_fd)
{
    return oct_syscall2(22, (uint64_t)source_fd, (uint64_t)target_fd);
}

#endif
