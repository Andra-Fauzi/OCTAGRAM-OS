#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

static const char child_message[] = "pipe child output\n";

int main(int argc, char **argv)
{
    if (argc == 4 && strcmp(argv[1], "child") == 0) {
        int read_fd = atoi(argv[2]);
        int write_fd = atoi(argv[3]);
        if (dup2(write_fd, STDOUT_FILENO) < 0 ||
            close(read_fd) < 0 ||
            close(write_fd) < 0) {
            return 10;
        }
        return write(STDOUT_FILENO, child_message,
                     sizeof(child_message) - 1) == sizeof(child_message) - 1
                   ? 0
                   : 11;
    }

    if (argc != 1) {
        return 2;
    }

    int descriptors[2];
    if (pipe(descriptors) < 0) {
        return 3;
    }

    char read_fd[16];
    char write_fd[16];
    snprintf(read_fd, sizeof(read_fd), "%d", descriptors[0]);
    snprintf(write_fd, sizeof(write_fd), "%d", descriptors[1]);
    char *child_argv[] = {argv[0], "child", read_fd, write_fd, NULL};
    pid_t child_pid;
    if (posix_spawn(&child_pid, argv[0], NULL, NULL, child_argv, environ) != 0) {
        return 4;
    }
    if (close(descriptors[1]) < 0) {
        return 5;
    }

    char output[sizeof(child_message) - 1];
    ssize_t bytes_read = read(descriptors[0], output, sizeof(output));
    if (bytes_read != (int64_t)sizeof(output)) {
        return 6;
    }
    if (close(descriptors[0]) < 0) {
        return 7;
    }

    int child_status = 0;
    if (waitpid(child_pid, &child_status, 0) != child_pid ||
        !WIFEXITED(child_status) || WEXITSTATUS(child_status) != 0) {
        return 8;
    }

    for (size_t i = 0; i < sizeof(output); ++i) {
        if (output[i] != child_message[i]) {
            return 9;
        }
    }
    puts("pipe/fork/execve/dup2 passed");
    return 0;
}
