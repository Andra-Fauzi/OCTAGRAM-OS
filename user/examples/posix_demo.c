#include "octagram.h"

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void)
{
    const char directory[] = "/posix-demo-dir";
    const char nested_file[] = "/posix-demo-dir/file.txt";
    if (mkdir(directory, 0700) != 0) {
        return 1;
    }
    struct stat metadata;
    if (stat(directory, &metadata) != 0 ||
        !S_ISDIR(metadata.st_mode) ||
        (metadata.st_mode & 0777) != 0700 ||
        metadata.st_size != 0) {
        return 2;
    }
    if (mkdir(directory, 0700) != -1 || errno != EEXIST ||
        stat(directory, NULL) != -1 || errno != EFAULT) {
        return 3;
    }

    char initial_cwd[256];
    char current_cwd[256];
    char short_cwd[4];
    if (getcwd(initial_cwd, sizeof(initial_cwd)) == NULL ||
        chdir(directory) != 0 ||
        getcwd(current_cwd, sizeof(current_cwd)) == NULL ||
        strcmp(current_cwd, directory) != 0) {
        return 4;
    }
    if (getcwd(short_cwd, sizeof(short_cwd)) != NULL || errno != ERANGE) {
        return 5;
    }
    if (chdir("..") != 0 ||
        getcwd(current_cwd, sizeof(current_cwd)) == NULL ||
        strcmp(current_cwd, "/") != 0 ||
        chdir(initial_cwd) != 0) {
        return 6;
    }

    int nested_fd = open(nested_file, O_CREAT | O_TRUNC | O_WRONLY, 0600);
    if (nested_fd < 0 || write(nested_fd, "x", 1) != 1 ||
        close(nested_fd) != 0) {
        return 7;
    }
    if (chmod(nested_file, 0640) != 0 ||
        stat(nested_file, &metadata) != 0 ||
        (metadata.st_mode & 0777) != 0640 ||
        chmod(nested_file, 01000) != -1 || errno != EINVAL ||
        chdir(nested_file) != -1 || errno != ENOTDIR) {
        return 8;
    }
    DIR *directory_stream = opendir(directory);
    if (directory_stream == NULL) {
        return 21;
    }
    int found_nested_file = 0;
    errno = 0;
    struct dirent *directory_entry;
    while ((directory_entry = readdir(directory_stream)) != NULL) {
        if (strcmp(directory_entry->d_name, "file.txt") == 0 &&
            directory_entry->d_type == DT_REG &&
            directory_entry->d_reclen == sizeof(*directory_entry)) {
            found_nested_file = 1;
        }
    }
    int directory_error = errno;
    int directory_close_result = closedir(directory_stream);
    if (directory_error != 0 || !found_nested_file || directory_close_result != 0) {
        return 22;
    }
    directory_stream = opendir(nested_file);
    if (directory_stream != NULL) {
        closedir(directory_stream);
        return 23;
    }
    if (errno != ENOTDIR) {
        return 23;
    }
    if (stat(nested_file, &metadata) != 0 ||
        !S_ISREG(metadata.st_mode) ||
        metadata.st_size != 1 ||
        unlink(nested_file) != 0 ||
        unlink(directory) != 0) {
        return 9;
    }

    const char expected[] = "posix io";
    char actual[sizeof(expected)] = {0};
    int fd = open("/posix-demo.txt", O_CREAT | O_TRUNC | O_RDWR, 0600);
    if (fd < 0) {
        return 10;
    }
    if (write(fd, expected, sizeof(expected) - 1) !=
            (ssize_t)(sizeof(expected) - 1) ||
        lseek(fd, 0, SEEK_SET) != 0 ||
        read(fd, actual, sizeof(expected) - 1) !=
            (ssize_t)(sizeof(expected) - 1) ||
        memcmp(actual, expected, sizeof(expected) - 1) != 0) {
        return 11;
    }
    if (ftruncate(fd, -1) != -1 || errno != EINVAL ||
        ftruncate(fd, 4) != 0 ||
        stat("/posix-demo.txt", &metadata) != 0 || metadata.st_size != 4 ||
        lseek(fd, 0, SEEK_SET) != 0 ||
        read(fd, actual, 4) != 4 || memcmp(actual, "posi", 4) != 0) {
        return 12;
    }
    if (ftruncate(fd, sizeof(expected) - 1) != 0 ||
        lseek(fd, 4, SEEK_SET) != 4 ||
        read(fd, actual, 4) != 4 ||
        actual[0] != '\0' || actual[1] != '\0' ||
        actual[2] != '\0' || actual[3] != '\0') {
        return 13;
    }
    if (close(fd) != 0 || unlink("/posix-demo.txt") != 0) {
        return 14;
    }

    char byte;
    if (read(-1, &byte, 1) != -1 || errno != EBADF) {
        return 15;
    }

    int descriptors[2];
    if (pipe(descriptors) != 0) {
        return 16;
    }
    if (write(descriptors[1], "p", 1) != 1 ||
        read(descriptors[0], &byte, 1) != 1 || byte != 'p') {
        return 17;
    }
    if (close(descriptors[0]) != 0 || close(descriptors[1]) != 0) {
        return 18;
    }

    if (getpid() <= 0 || isatty(OCT_STDOUT) != 1) {
        return 19;
    }
    static const char message[] =
        "POSIX cwd, stat, directory, permissions, truncate, file, and pipe APIs passed\n";
    return write(OCT_STDOUT, message, sizeof(message) - 1) ==
                   (ssize_t)(sizeof(message) - 1)
               ? 0
               : 20;
}
