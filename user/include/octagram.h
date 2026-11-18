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
#define OCT_O_NONBLOCK (1u << 11)
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
#define OCT_IPV4(a, b, c, d) \
    ((((uint32_t)(a) & 0xffu) << 24) | (((uint32_t)(b) & 0xffu) << 16) | \
     (((uint32_t)(c) & 0xffu) << 8) | ((uint32_t)(d) & 0xffu))

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

static inline int64_t oct_yield(void)
{
    return oct_syscall0(65);
}

static inline int64_t oct_udp_socket(void)
{
    return oct_syscall0(66);
}

static inline int64_t oct_udp_bind(int fd, uint16_t port)
{
    return oct_syscall2(67, (uint64_t)fd, port);
}

static inline int64_t oct_udp_sendto(int fd, uint32_t destination,
                                     uint16_t port, const void *buffer,
                                     size_t length)
{
    return oct_syscall6(68, (uint64_t)fd, destination, port,
                        (uint64_t)(uintptr_t)buffer, length, 0);
}

static inline int64_t oct_udp_recvfrom(int fd, void *buffer, size_t length,
                                       uint8_t source_ip[4],
                                       uint16_t *source_port)
{
    return oct_syscall6(69, (uint64_t)fd,
                        (uint64_t)(uintptr_t)buffer, length,
                        (uint64_t)(uintptr_t)source_ip,
                        (uint64_t)(uintptr_t)source_port, 0);
}

static inline int64_t oct_dns_server(uint8_t address[4])
{
    return oct_syscall1(70, (uint64_t)(uintptr_t)address);
}

static inline int64_t oct_socket(int domain, int type, int protocol)
{
    return oct_syscall3(71, (uint64_t)(int64_t)domain,
                        (uint64_t)(int64_t)type,
                        (uint64_t)(int64_t)protocol);
}

static inline int64_t oct_bind(int fd, const void *address, uint32_t length)
{
    return oct_syscall3(72, (uint64_t)(int64_t)fd,
                        (uint64_t)(uintptr_t)address, length);
}

static inline int64_t oct_listen(int fd, int backlog)
{
    return oct_syscall2(73, (uint64_t)(int64_t)fd,
                        (uint64_t)(int64_t)backlog);
}

static inline int64_t oct_accept(int fd, void *address, uint32_t *length)
{
    return oct_syscall3(74, (uint64_t)(int64_t)fd,
                        (uint64_t)(uintptr_t)address,
                        (uint64_t)(uintptr_t)length);
}

static inline int64_t oct_connect(int fd, const void *address, uint32_t length)
{
    return oct_syscall3(75, (uint64_t)(int64_t)fd,
                        (uint64_t)(uintptr_t)address, length);
}

static inline int64_t oct_send(int fd, const void *buffer, size_t length, int flags)
{
    return oct_syscall4(76, (uint64_t)(int64_t)fd,
                        (uint64_t)(uintptr_t)buffer, length,
                        (uint64_t)(unsigned int)flags);
}

static inline int64_t oct_recv(int fd, void *buffer, size_t length, int flags)
{
    return oct_syscall4(77, (uint64_t)(int64_t)fd,
                        (uint64_t)(uintptr_t)buffer, length,
                        (uint64_t)(unsigned int)flags);
}

static inline int64_t oct_shutdown(int fd, int how)
{
    return oct_syscall2(78, (uint64_t)(int64_t)fd,
                        (uint64_t)(int64_t)how);
}

static inline int64_t oct_getsockname(int fd, void *address, uint32_t *length)
{
    return oct_syscall3(79, (uint64_t)(int64_t)fd,
                        (uint64_t)(uintptr_t)address,
                        (uint64_t)(uintptr_t)length);
}

static inline int64_t oct_getpeername(int fd, void *address, uint32_t *length)
{
    return oct_syscall3(80, (uint64_t)(int64_t)fd,
                        (uint64_t)(uintptr_t)address,
                        (uint64_t)(uintptr_t)length);
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

static inline int64_t oct_pipe2(int *descriptors, int64_t flags)
{
    return oct_syscall2(98, (uint64_t)(uintptr_t)descriptors, (uint64_t)flags);
}

static inline int64_t oct_dup2(int source_fd, int target_fd)
{
    return oct_syscall2(22, (uint64_t)source_fd, (uint64_t)target_fd);
}

static inline int64_t oct_ioctl(int fd, uint64_t request, void *argp)
{
    return oct_syscall3(81, (uint64_t)fd, request, (uint64_t)(uintptr_t)argp);
}

struct oct_timespec {
    int64_t tv_sec;
    int64_t tv_nsec;
};

struct oct_timeval {
    int64_t tv_sec;
    int64_t tv_usec;
};

struct oct_utsname {
    char sysname[65];
    char nodename[65];
    char release[65];
    char version[65];
    char machine[65];
    char domainname[65];
};

struct oct_pollfd {
    int32_t fd;
    int16_t events;
    int16_t revents;
};

#define OCT_POLLIN  0x0001
#define OCT_POLLOUT 0x0004
#define OCT_POLLERR 0x0008
#define OCT_POLLHUP 0x0010
#define OCT_POLLNVAL 0x0020

#define OCT_SIG_DFL ((void *)0)
#define OCT_SIG_IGN ((void *)1)

static inline int64_t oct_getuid(void)
{
    return oct_syscall0(82);
}

static inline int64_t oct_getgid(void)
{
    return oct_syscall0(83);
}

static inline int64_t oct_geteuid(void)
{
    return oct_syscall0(84);
}

static inline int64_t oct_getegid(void)
{
    return oct_syscall0(85);
}

static inline int64_t oct_uname(struct oct_utsname *name)
{
    return oct_syscall1(86, (uint64_t)(uintptr_t)name);
}

static inline int64_t oct_brk(uint64_t address)
{
    return oct_syscall1(88, address);
}

static inline int64_t oct_gettimeofday(struct oct_timeval *time_value)
{
    return oct_syscall1(89, (uint64_t)(uintptr_t)time_value);
}

static inline int64_t oct_nanosleep(const struct oct_timespec *request)
{
    return oct_syscall1(90, (uint64_t)(uintptr_t)request);
}

static inline int64_t oct_kill(int64_t pid, int64_t signal_number)
{
    return oct_syscall2(91, (uint64_t)pid, (uint64_t)signal_number);
}

static inline int64_t oct_sigaction(int64_t signal_number, const void *action,
                                    void *old_action)
{
    return oct_syscall3(92, (uint64_t)signal_number,
                        (uint64_t)(uintptr_t)action,
                        (uint64_t)(uintptr_t)old_action);
}

static inline int64_t oct_sigprocmask(int64_t how, const void *set,
                                      void *old_set)
{
    return oct_syscall3(93, (uint64_t)how, (uint64_t)(uintptr_t)set,
                        (uint64_t)(uintptr_t)old_set);
}

static inline int64_t oct_futex(uint32_t *address, int64_t operation,
                                uint32_t value)
{
    return oct_syscall3(94, (uint64_t)(uintptr_t)address,
                        (uint64_t)operation, value);
}

static inline int64_t oct_futex_wait_timeout(uint32_t *address, uint32_t value,
                                             const struct oct_timespec *timeout)
{
    return oct_syscall4(94, (uint64_t)(uintptr_t)address, 0, (uint64_t)value,
                        (uint64_t)(uintptr_t)timeout);
}

static inline int64_t oct_setsockopt(int64_t fd, int64_t level,
                                     int64_t option_name,
                                     const void *option_value,
                                     uint32_t option_length)
{
    return oct_syscall6(95, (uint64_t)fd, (uint64_t)level,
                        (uint64_t)option_name,
                        (uint64_t)(uintptr_t)option_value, option_length, 0);
}

static inline int64_t oct_getsockopt(int64_t fd, int64_t level,
                                     int64_t option_name, void *option_value,
                                     uint32_t *option_length)
{
    return oct_syscall6(96, (uint64_t)fd, (uint64_t)level,
                        (uint64_t)option_name,
                        (uint64_t)(uintptr_t)option_value,
                        (uint64_t)(uintptr_t)option_length, 0);
}

static inline int64_t oct_poll(struct oct_pollfd *fds, uint64_t nfds,
                               int64_t timeout_milliseconds)
{
    return oct_syscall3(97, (uint64_t)(uintptr_t)fds, nfds,
                        (uint64_t)timeout_milliseconds);
}

static inline int64_t oct_sysconf(int name)
{
    return oct_syscall1(108, (uint64_t)(int64_t)name);
}

static inline int64_t oct_sigreturn(void)
{
    return oct_syscall0(113);
}

static inline int64_t oct_sigpending(void *set)
{
    return oct_syscall1(114, (uint64_t)(uintptr_t)set);
}

static inline int64_t oct_sigsuspend(const void *set)
{
    return oct_syscall1(115, (uint64_t)(uintptr_t)set);
}

static inline int64_t oct_signal(int64_t signal_number, void *handler)
{
    return oct_syscall2(116, (uint64_t)signal_number,
                        (uint64_t)(uintptr_t)handler);
}

static inline int64_t oct_select(uint64_t nfds, void *readfds,
                                 void *writefds, void *exceptfds,
                                 struct oct_timeval *timeout)
{
    return oct_syscall6(117, nfds, (uint64_t)(uintptr_t)readfds,
                        (uint64_t)(uintptr_t)writefds,
                        (uint64_t)(uintptr_t)exceptfds,
                        (uint64_t)(uintptr_t)timeout, 0);
}

static inline int64_t oct_getppid(void)
{
    return oct_syscall0(87);
}

static inline int64_t oct_clock_getres(int64_t clock_id,
                                       struct oct_timespec *resolution)
{
    return oct_syscall2(118, (uint64_t)clock_id,
                        (uint64_t)(uintptr_t)resolution);
}

static inline int64_t oct_fsync(int64_t fd)
{
    return oct_syscall1(119, (uint64_t)fd);
}

static inline int64_t oct_fdatasync(int64_t fd)
{
    return oct_syscall1(120, (uint64_t)fd);
}

static inline int64_t oct_getrandom(void *buffer, uint64_t length,
                                     uint64_t flags)
{
    return oct_syscall3(121, (uint64_t)(uintptr_t)buffer, length, flags);
}

static inline int64_t oct_epoll_create1(int64_t flags)
{
    return oct_syscall1(122, (uint64_t)flags);
}

static inline int64_t oct_epoll_ctl(int64_t epfd, int64_t operation,
                                    int64_t fd, void *event)
{
    return oct_syscall4(123, (uint64_t)epfd, (uint64_t)operation, (uint64_t)fd,
                        (uint64_t)(uintptr_t)event);
}

static inline int64_t oct_epoll_wait(int64_t epfd, void *events,
                                     int64_t maxevents, int64_t timeout_ms)
{
    return oct_syscall4(124, (uint64_t)epfd, (uint64_t)(uintptr_t)events,
                        (uint64_t)maxevents, (uint64_t)timeout_ms);
}

static inline int64_t oct_eventfd2(uint64_t initial_value, int64_t flags)
{
    return oct_syscall2(100, initial_value, (uint64_t)flags);
}

static inline int64_t oct_timerfd_create(int64_t clock_id, int64_t flags)
{
    return oct_syscall2(101, (uint64_t)clock_id, (uint64_t)flags);
}

static inline int64_t oct_timerfd_settime(int64_t fd, int64_t flags,
                                          const void *new_value,
                                          void *old_value)
{
    return oct_syscall4(125, (uint64_t)fd, (uint64_t)flags,
                        (uint64_t)(uintptr_t)new_value,
                        (uint64_t)(uintptr_t)old_value);
}

static inline int64_t oct_timerfd_gettime(int64_t fd, void *current_value)
{
    return oct_syscall2(126, (uint64_t)fd, (uint64_t)(uintptr_t)current_value);
}

static inline int64_t oct_socketpair(int64_t domain, int64_t socket_type,
                                     int64_t protocol, void *addresses)
{
    return oct_syscall4(127, (uint64_t)domain, (uint64_t)socket_type,
                        (uint64_t)protocol, (uint64_t)(uintptr_t)addresses);
}

static inline int64_t oct_clock_nanosleep(int64_t clock_id, int64_t flags,
                                          const struct oct_timespec *request,
                                          struct oct_timespec *remaining)
{
    return oct_syscall4(128, (uint64_t)clock_id, (uint64_t)flags,
                        (uint64_t)(uintptr_t)request,
                        (uint64_t)(uintptr_t)remaining);
}

static inline int64_t oct_pread(int fd, void *buffer, uint64_t count,
                                uint64_t offset)
{
    return oct_syscall4(102, (uint64_t)(int64_t)fd,
                        (uint64_t)(uintptr_t)buffer, count, offset);
}

static inline int64_t oct_pwrite(int fd, const void *buffer, uint64_t count,
                                 uint64_t offset)
{
    return oct_syscall4(103, (uint64_t)(int64_t)fd,
                        (uint64_t)(uintptr_t)buffer, count, offset);
}

static inline int64_t oct_readv(int fd, const void *iov, uint64_t iov_count)
{
    return oct_syscall3(105, (uint64_t)(int64_t)fd,
                        (uint64_t)(uintptr_t)iov, iov_count);
}

static inline int64_t oct_writev(int fd, const void *iov, uint64_t iov_count)
{
    return oct_syscall3(104, (uint64_t)(int64_t)fd,
                        (uint64_t)(uintptr_t)iov, iov_count);
}

static inline int64_t oct_dup3(int source_fd, int target_fd, int64_t flags)
{
    return oct_syscall3(99, (uint64_t)(int64_t)source_fd,
                        (uint64_t)(int64_t)target_fd, (uint64_t)flags);
}

static inline int64_t oct_getrlimit(int64_t resource, void *limit)
{
    return oct_syscall2(109, (uint64_t)resource, (uint64_t)(uintptr_t)limit);
}

static inline int64_t oct_setrlimit(int64_t resource, const void *limit)
{
    return oct_syscall2(110, (uint64_t)resource, (uint64_t)(uintptr_t)limit);
}

static inline int64_t oct_getrusage(int64_t who, void *usage)
{
    return oct_syscall2(111, (uint64_t)who, (uint64_t)(uintptr_t)usage);
}

static inline int64_t oct_times(void *process_times)
{
    return oct_syscall1(112, (uint64_t)(uintptr_t)process_times);
}

static inline int64_t oct_getpgrp(void)
{
    return oct_syscall0(129);
}

static inline int64_t oct_setpgid(uint64_t pid, uint64_t process_group)
{
    return oct_syscall2(130, pid, process_group);
}

static inline int64_t oct_getsid(uint64_t pid)
{
    return oct_syscall1(131, pid);
}

static inline int64_t oct_setsid(void)
{
    return oct_syscall0(132);
}

#endif
