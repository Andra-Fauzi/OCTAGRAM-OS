#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>

int main(int argc, char **argv)
{
    if (argc == 2 && strcmp(argv[1], "child") == 0) {
        char *value = getenv("OCTAGRAM_SPAWN_PROBE");
        return value != NULL && strcmp(value, "child-environment") == 0 &&
                       getenv("PATH") == NULL
                   ? 0
                   : 1;
    }
    if (argc != 1) {
        return 2;
    }

    char *arguments[] = {argv[0], "child", NULL};
    char *environment[] = {"OCTAGRAM_SPAWN_PROBE=child-environment", NULL};
    char *parent_value = getenv("OCTAGRAM_SPAWN_PROBE");
    pid_t child_pid;
    int error = posix_spawn(&child_pid, argv[0], NULL, NULL, arguments,
                            environment);
    if (error != 0) {
        return 3;
    }

    int status;
    if (waitpid(child_pid, &status, 0) != child_pid ||
        !WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        return 4;
    }
    if (getenv("OCTAGRAM_SPAWN_PROBE") != parent_value) {
        return 5;
    }
    return printf("posix_spawn environment/status passed\n") < 0 ? 6 : 0;
}
