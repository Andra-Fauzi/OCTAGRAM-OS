#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

int main(int argc, char **argv)
{
    if (argc == 4 && strcmp(argv[1], "child") == 0) {
        int read_fd = atoi(argv[2]);
        int write_fd = atoi(argv[3]);
        char ready = 'r';
        char release;
        if (write(write_fd, &ready, 1) != 1 ||
            read(read_fd, &release, 1) != 1 || release != 'x') {
            return 1;
        }
        char *value = getenv("OCTAGRAM_SPAWN_PROBE");
        return value != NULL && strcmp(value, "child-environment") == 0 &&
                       getenv("PATH") == NULL
                   ? 0
                   : 1;
    }
    if (argc != 1) {
        return 2;
    }

    int pipe_fds[2];
    if (pipe(pipe_fds) != 0) {
        return 3;
    }
    char read_fd[16];
    char write_fd[16];
    snprintf(read_fd, sizeof(read_fd), "%d", pipe_fds[0]);
    snprintf(write_fd, sizeof(write_fd), "%d", pipe_fds[1]);
    char *arguments[] = {argv[0], "child", read_fd, write_fd, NULL};
    char *environment[] = {"OCTAGRAM_SPAWN_PROBE=child-environment", NULL};
    char *parent_value = getenv("OCTAGRAM_SPAWN_PROBE");
    pid_t child_pid;
    int error = posix_spawn(&child_pid, argv[0], NULL, NULL, arguments,
                            environment);
    if (error != 0) {
        return 4;
    }

    char ready;
    if (read(pipe_fds[0], &ready, 1) != 1 || ready != 'r') {
        return 5;
    }

    int status = 0x1234;
    if (waitpid(child_pid, &status, WNOHANG) != 0 || status != 0x1234) {
        return 6;
    }
    char release = 'x';
    if (write(pipe_fds[1], &release, 1) != 1) {
        return 7;
    }

    if (waitpid(child_pid, &status, 0) != child_pid ||
        !WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        return 8;
    }
    if (getenv("OCTAGRAM_SPAWN_PROBE") != parent_value) {
        return 9;
    }
    return printf("posix_spawn environment/status/WNOHANG passed\n") < 0 ? 10 : 0;
}
