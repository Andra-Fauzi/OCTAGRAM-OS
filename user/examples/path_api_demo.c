#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void)
{
    const char *directory = "/tmp/path-api-demo";
    const char *file = "/tmp/path-api-demo/file";
    const char *link = "/tmp/path-api-demo/link";
    if (mkdir(directory, 0700) != 0) {
        return 1;
    }

    int fd = open(file, O_CREAT | O_TRUNC | O_RDWR, 0600);
    if (fd < 0 || close(fd) != 0) {
        return 2;
    }
    if (symlink("file", link) != 0) {
        return 3;
    }

    char target[8] = {0};
    if (readlink(link, target, sizeof(target)) != 4 ||
        target[0] != 'f' || target[1] != 'i' ||
        target[2] != 'l' || target[3] != 'e') {
        return 4;
    }
    char short_target[2];
    if (readlink(link, short_target, sizeof(short_target)) != 2 ||
        short_target[0] != 'f' || short_target[1] != 'i') {
        return 5;
    }

    struct stat status;
    if (stat(link, &status) != 0 || !S_ISREG(status.st_mode)) {
        return 6;
    }
    if (lstat(link, &status) != 0 || !S_ISLNK(status.st_mode) ||
        status.st_size != 4) {
        return 12;
    }
    if (readlink(file, target, sizeof(target)) != -1 || errno != EINVAL) {
        return 7;
    }
    if (unlink(directory) != -1 || errno != EISDIR) {
        return 8;
    }
    if (rmdir(file) != -1 || errno != ENOTDIR) {
        return 9;
    }
    if (rmdir(directory) != -1 || errno != ENOTEMPTY) {
        return 10;
    }

    if (unlink(link) != 0 || stat(file, &status) != 0 ||
        unlink(file) != 0 || rmdir(directory) != 0) {
        return 11;
    }
    puts("rmdir/symlink/readlink passed");
    return 0;
}
