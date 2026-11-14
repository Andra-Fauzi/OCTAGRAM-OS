#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

int main(int argc, char **argv)
{
    if (argc == 2 && strcmp(argv[1], "after-exec") == 0) {
        const char *value = getenv("OCTAGRAM_EXECVE_TEST");
        if (value == NULL || strcmp(value, "passed") != 0) {
            return 20;
        }
        if (fcntl(3, F_GETFD) != -1 || errno != EBADF) {
            return 21;
        }
        struct stat status;
        if (fstat(4, &status) != 0) {
            return 22;
        }
        close(4);
        unlink("/tmp/execve-demo-file");
        puts("execve replaced image and preserved process state");
        return 0;
    }

    int close_on_exec = open("/tmp/execve-demo-file",
                             O_CREAT | O_TRUNC | O_RDWR, 0600);
    int inherited = open("/tmp/execve-demo-file", O_RDONLY);
    if (close_on_exec != 3 || inherited != 4 ||
        fcntl(close_on_exec, F_SETFD, FD_CLOEXEC) != 0) {
        return 1;
    }

    char *arguments[] = {"execve-demo", "after-exec", NULL};
    char *environment[] = {"OCTAGRAM_EXECVE_TEST=passed", NULL};
    if (execve("/does-not-exist", arguments, environment) != -1 ||
        errno != ENOENT) {
        return 2;
    }
    if (execve("/tmp", arguments, environment) != -1 || errno != ENOEXEC) {
        return 3;
    }
    execve("/bin/execve-demo", arguments, environment);
    return 10 + errno;
}
