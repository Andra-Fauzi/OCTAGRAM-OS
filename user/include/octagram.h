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

#define OCT_SEEK_SET 0
#define OCT_SEEK_CUR 1
#define OCT_SEEK_END 2

#define OCT_PROT_READ  (1u << 0)
#define OCT_PROT_WRITE (1u << 1)
#define OCT_PROT_EXEC  (1u << 2)

#define OCT_PIPE_CAPACITY 4096
#define OCT_MAX_SYSCALL_IO 4096
#define OCT_WAIT_ANY UINT64_MAX

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

static inline int64_t oct_mkdir(const char *path)
{
    return oct_syscall1(7, (uint64_t)(uintptr_t)path);
}

static inline int64_t oct_unlink(const char *path)
{
    return oct_syscall1(8, (uint64_t)(uintptr_t)path);
}

static inline int64_t oct_ftruncate(int fd, size_t length)
{
    return oct_syscall2(9, (uint64_t)fd, length);
}

static inline int64_t oct_brk(uintptr_t address)
{
    return oct_syscall1(10, address);
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

static inline int64_t oct_putpixel(uint64_t x, uint64_t y, uint32_t rgb)
{
    return oct_syscall3(15, x, y, rgb);
}

static inline int64_t oct_mmap(uintptr_t address, size_t length,
                               uint64_t protection)
{
    return oct_syscall3(16, address, length, protection);
}

static inline int64_t oct_munmap(uintptr_t address, size_t length)
{
    return oct_syscall2(17, address, length);
}

static inline int64_t oct_mmap_file(int fd, uint64_t offset, size_t length)
{
    return oct_syscall3(18, (uint64_t)fd, offset, length);
}

static inline int64_t oct_spawn(const char *path, const char *const argv[],
                                size_t argc)
{
    return oct_syscall3(19, (uint64_t)(uintptr_t)path,
                        (uint64_t)(uintptr_t)argv, argc);
}

static inline int64_t oct_spawn_env(const char *path, const char *const argv[],
                                    size_t argc, char *const environment[])
{
    return oct_syscall4(24, (uint64_t)(uintptr_t)path,
                        (uint64_t)(uintptr_t)argv, argc,
                        (uint64_t)(uintptr_t)environment);
}

static inline int64_t oct_waitpid(uint64_t pid, uint64_t *status)
{
    return oct_syscall2(20, pid, (uint64_t)(uintptr_t)status);
}

static inline int64_t oct_pipe(int descriptors[2])
{
    return oct_syscall1(21, (uint64_t)(uintptr_t)descriptors);
}

static inline int64_t oct_dup2(int source_fd, int target_fd)
{
    return oct_syscall2(22, (uint64_t)source_fd, (uint64_t)target_fd);
}

static inline int64_t oct_set_environ(char *const environment[])
{
    return oct_syscall1(23, (uint64_t)(uintptr_t)environment);
}

#endif
