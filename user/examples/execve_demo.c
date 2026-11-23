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

        /* An empty PATH is not the same as an unset PATH: it searches ".". */
        char *empty_path_arguments[] = {"execve-demo", "after-empty-path", NULL};
        if (chdir("/bin") != 0 || setenv("PATH", "", 1) != 0) {
            return 23;
        }
        execvp("execve-demo", empty_path_arguments);
        return 24 + errno;
    }
    if (argc == 2 && strcmp(argv[1], "after-empty-path") == 0) {
        const char *value = getenv("OCTAGRAM_EXECVE_TEST");
        if (value == NULL || strcmp(value, "passed") != 0) {
            return 25;
        }
        if (fcntl(3, F_GETFD) != -1 || errno != EBADF) {
            return 26;
        }
        struct stat status;
        if (fstat(4, &status) != 0) {
            return 27;
        }
        close(4);
        unlink("/tmp/execve-demo-file");
        rmdir("/tmp/execve-demo");
        puts("EXECVP-PATH-SEARCH-PASS: skipped directory and handled empty PATH");
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
    /* The first PATH component is a directory with the same basename as
     * the executable. execvp must skip it and continue to /bin, not loop. */
    (void)rmdir("/tmp/execve-demo");
    if (mkdir("/tmp/execve-demo", 0700) != 0 ||
        setenv("PATH", "/tmp:/bin", 1) != 0 ||
        setenv("OCTAGRAM_EXECVE_TEST", "passed", 1) != 0) {
        return 4;
    }
    execvp("execve-demo", arguments);
    return 30 + errno;
}
