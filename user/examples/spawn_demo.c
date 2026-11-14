#include "octagram.h"

int main(int argc, char **argv)
{
    static const char usage[] = "usage: spawn_demo <child-elf>\n";
    static const char spawn_error[] = "spawn_demo: spawn failed\n";
    static const char wait_error[] = "spawn_demo: waitpid failed\n";
    static const char child_error[] = "spawn_demo: child failed\n";
    static const char success[] = "spawn_demo: child exited successfully\n";

    if (argc != 2) {
        oct_write(OCT_STDERR, usage, sizeof(usage) - 1);
        return 2;
    }

    const char *child_argv[] = {argv[1], 0};
    int64_t child_pid = oct_spawn(argv[1], child_argv, 1);
    if (child_pid < 0) {
        oct_write(OCT_STDERR, spawn_error, sizeof(spawn_error) - 1);
        return 3;
    }

    uint64_t status = 0;
    if (oct_waitpid(OCT_WAIT_ANY, &status) != child_pid) {
        oct_write(OCT_STDERR, wait_error, sizeof(wait_error) - 1);
        return 4;
    }
    if (status != 0) {
        oct_write(OCT_STDERR, child_error, sizeof(child_error) - 1);
        return 5;
    }

    oct_write(OCT_STDOUT, success, sizeof(success) - 1);
    return 0;
}
