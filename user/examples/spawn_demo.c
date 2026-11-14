#include <spawn.h>
#include <stdio.h>
#include <sys/wait.h>
#include <unistd.h>

int main(int argc, char **argv)
{
    if (argc != 2) {
        puts("usage: spawn_demo <child-elf>");
        return 2;
    }

    char *child_argv[] = {argv[1], NULL};
    pid_t child_pid;
    int error = posix_spawn(&child_pid, argv[1], NULL, NULL, child_argv, environ);
    if (error != 0) {
        printf("spawn_demo: spawn failed: %d\n", error);
        return 3;
    }

    int status = 0;
    if (waitpid(child_pid, &status, 0) != child_pid) {
        puts("spawn_demo: waitpid failed");
        return 4;
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        puts("spawn_demo: child failed");
        return 5;
    }

    puts("spawn_demo: child exited successfully");
    return 0;
}
