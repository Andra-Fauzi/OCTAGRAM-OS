#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <sys/stat.h>
#include <unistd.h>
#include <sys/uio.h>

int main(void)
{
    const char *path = "/tmp/file-api-demo";
    const char *renamed = "/tmp/file-api-demo-renamed";
    mode_t previous_mask = umask(0077);
    int fd = open(path, O_CREAT | O_TRUNC | O_RDWR, 0666);
    umask(previous_mask);
    if (fd < 0) {
        return 1;
    }

    struct stat status;
    if (fstat(fd, &status) != 0) {
        close(fd);
        unlink(path);
        return 2;
    }
    if (!S_ISREG(status.st_mode)) {
        close(fd);
        unlink(path);
        return 20;
    }
    if ((status.st_mode & 0777) != 0600) {
        close(fd);
        unlink(path);
        return 30 + (status.st_mode & 0777);
    }
    if (fchmod(fd, 0640) != 0 || fstat(fd, &status) != 0 ||
        (status.st_mode & 0777) != 0640) {
        close(fd);
        unlink(path);
        return 31;
    }
    if (fchmod(-1, 0600) != -1) {
        close(fd);
        unlink(path);
        return 32;
    }
    if (errno != EBADF) {
        close(fd);
        unlink(path);
        return 33;
    }
    /* Special permission bits are currently discarded by the kernel. */
    if (fchmod(fd, 01000) != 0) {
        close(fd);
        unlink(path);
        return 34;
    }
    /* Values that cannot be represented by the kernel's u16 mode are invalid. */
    if (fchmod(fd, 0x10000) != -1) {
        close(fd);
        unlink(path);
        return 35;
    }
    if (errno != EINVAL) {
        close(fd);
        unlink(path);
        return 36;
    }
    if (access(path, R_OK | W_OK) != 0) {
        int error = errno;
        close(fd);
        unlink(path);
        return 40 + error;
    }
    if (access(path, X_OK) == 0) {
        close(fd);
        unlink(path);
        return 41;
    }

    int duplicate = dup(fd);
    int fcntl_duplicate = fcntl(fd, F_DUPFD, 5);
    if (duplicate < 0 || fcntl_duplicate < 0 ||
        fcntl(duplicate, F_SETFD, FD_CLOEXEC) != 0 ||
        fcntl(duplicate, F_GETFD) != FD_CLOEXEC ||
        fcntl(fd, F_SETFL, O_APPEND) != 0 ||
        (fcntl(fd, F_GETFL) & O_APPEND) == 0 ||
        (fcntl(duplicate, F_GETFL) & O_APPEND) == 0 ||
        write(duplicate, "ok", 2) != 2 || lseek(fd, 0, SEEK_CUR) != 2) {
        close(fd);
        unlink(path);
        return 3;
    }

    char positional[3] = {0, 0, 0};
    if (fcntl(fd, F_SETFL, 0) != 0 ||
        pwrite(duplicate, "X", 1, 0) != 1 ||
        lseek(fd, 0, SEEK_CUR) != 2 ||
        pread(fd, positional, 2, 0) != 2 ||
        positional[0] != 'X' || positional[1] != 'k' ||
        lseek(duplicate, 0, SEEK_CUR) != 2) {
        close(fd);
        unlink(path);
        return 6;
    }

    struct iovec append_vector = {(void *)"!", 1};
    if (lseek(fd, 0, SEEK_END) != 2 ||
        pwritev2(fd, &append_vector, 1, (off_t)-1, 0) != 1 ||
        lseek(fd, 0, SEEK_CUR) != 3 ||
        pwritev2(fd, &append_vector, 1, 0, 1) != -1 ||
        errno != ENOTSUP ||
        preadv2(fd, &append_vector, 1, 0, 1) != -1 ||
        errno != ENOTSUP ||
        preadv2(fd, &append_vector, 1, (off_t)-2, 0) != -1 ||
        errno != EINVAL) {
        close(fd);
        unlink(path);
        return 7;
    }

    if (rename(path, renamed) != 0 || access(path, F_OK) == 0 ||
        access(renamed, F_OK) != 0 || fstat(fd, &status) != 0 ||
        status.st_size != 3) {
        close(fd);
        unlink(renamed);
        return 4;
    }

    close(fcntl_duplicate);
    close(duplicate);
    close(fd);
    if (unlink(renamed) != 0) {
        return 5;
    }
    return puts("fstat/access/rename/umask/dup/fcntl/pread/pwrite passed") < 0 ? 6 : 0;
}
