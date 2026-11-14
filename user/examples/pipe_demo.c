#include "octagram.h"

static const char child_message[] = "pipe child output\n";
static const char success_message[] = "pipe/spawn/dup2 passed\n";

int main(int argc, char **argv)
{
    if (argc == 2 && argv[1][0] == 'c' && argv[1][1] == 'h' &&
        argv[1][2] == 'i' && argv[1][3] == 'l' && argv[1][4] == 'd' &&
        argv[1][5] == '\0') {
        if (oct_dup2(4, OCT_STDOUT) < 0 ||
            oct_close(3) < 0 ||
            oct_close(4) < 0) {
            return 10;
        }
        return oct_write(OCT_STDOUT, child_message,
                         sizeof(child_message) - 1) == sizeof(child_message) - 1
                   ? 0
                   : 11;
    }

    if (argc != 1) {
        return 2;
    }

    int descriptors[2];
    if (oct_pipe(descriptors) < 0 || descriptors[0] != 3 || descriptors[1] != 4) {
        return 3;
    }

    const char *child_argv[] = {argv[0], "child", 0};
    int64_t child_pid = oct_spawn(argv[0], child_argv, 2);
    if (child_pid < 0) {
        return 4;
    }
    if (oct_close(descriptors[1]) < 0) {
        return 5;
    }

    char output[sizeof(child_message) - 1];
    int64_t bytes_read = oct_read(descriptors[0], output, sizeof(output));
    if (bytes_read != (int64_t)sizeof(output)) {
        return 6;
    }
    if (oct_close(descriptors[0]) < 0) {
        return 7;
    }

    uint64_t child_status = 0;
    if (oct_waitpid((uint64_t)child_pid, &child_status) != child_pid ||
        child_status != 0) {
        return 8;
    }

    for (size_t i = 0; i < sizeof(output); ++i) {
        if (output[i] != child_message[i]) {
            return 9;
        }
    }
    oct_write(OCT_STDOUT, success_message, sizeof(success_message) - 1);
    return 0;
}
