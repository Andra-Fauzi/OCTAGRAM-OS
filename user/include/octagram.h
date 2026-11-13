#ifndef OCTAGRAM_H
#define OCTAGRAM_H

#include <stddef.h>
#include <stdint.h>

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

static inline int64_t oct_open(const char *path, uint64_t flags)
{
    return oct_syscall2(3, (uint64_t)(uintptr_t)path, flags);
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

#endif
