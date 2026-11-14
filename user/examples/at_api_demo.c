#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void)
{
    const char *directory = "/tmp/at-api-demo";
    if (mkdir(directory, 0700) != 0) {
        return 1;
    }

    DIR *stream = opendir(directory);
    if (stream == NULL) {
        rmdir(directory);
        return 2;
    }
    int dirfd_value = dirfd(stream);
    if (dirfd_value < 0) {
        closedir(stream);
        rmdir(directory);
        return 3;
    }

    int fd = openat(dirfd_value, "note", O_CREAT | O_RDWR, 0600);
    if (fd < 0 || close(fd) != 0) {
        closedir(stream);
        rmdir(directory);
        return 4;
    }
    struct stat status;
    if (fstatat(dirfd_value, "note", &status, 0) != 0 ||
        !S_ISREG(status.st_mode) || (status.st_mode & 0777) != 0600) {
        return 5;
    }
    if (fstatat(dirfd_value, "note", &status, AT_SYMLINK_NOFOLLOW) != 0 ||
        !S_ISREG(status.st_mode)) {
        return 6;
    }
    if (mkdirat(dirfd_value, "subdir", 0700) != 0 ||
        unlinkat(dirfd_value, "subdir", AT_REMOVEDIR) != 0) {
        return 7;
    }
    if (symlinkat("note", dirfd_value, "link") != 0) {
        return 8;
    }
    char target[8] = {0};
    if (readlinkat(dirfd_value, "link", target, sizeof(target)) != 4 ||
        target[0] != 'n' || target[1] != 'o' ||
        target[2] != 't' || target[3] != 'e') {
        return 9;
    }
    if (fstatat(dirfd_value, "link", &status, AT_SYMLINK_NOFOLLOW) != 0 ||
        !S_ISLNK(status.st_mode)) {
        return 10;
    }
    if (renameat(dirfd_value, "note", dirfd_value, "renamed") != 0 ||
        unlinkat(dirfd_value, "link", 0) != 0 ||
        unlinkat(dirfd_value, "renamed", 0) != 0 ||
        closedir(stream) != 0 || rmdir(directory) != 0) {
        return 11;
    }

    puts("openat/fstatat/mkdirat/unlinkat/renameat/readlinkat/symlinkat passed");
    return 0;
}
