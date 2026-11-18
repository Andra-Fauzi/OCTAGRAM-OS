#include <errno.h>
#include <fcntl.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

#define ACTION_PATH "/tmp/spawn-actions.txt"
static const char action_message[] = "posix_spawn file actions\n";

static int wait_for_child(pid_t child)
{
    int status = 0;
    return waitpid(child, &status, 0) == child && WIFEXITED(status) &&
                   WEXITSTATUS(status) == 0
               ? 0
               : 1;
}

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
    if (argc == 2 && strcmp(argv[1], "action-child") == 0) {
        if (write(STDOUT_FILENO, action_message, sizeof(action_message) - 1) !=
            (ssize_t)(sizeof(action_message) - 1)) {
            return 1;
        }
        /* POSIX_SPAWN_SETPGROUP with pgroup 0 puts the child in its own group. */
        return getpgid(0) == getpid() ? 0 : 1;
    }
    if (argc == 2 && strcmp(argv[1], "resetids-child") == 0) {
        /* POSIX_SPAWN_RESETIDS: id efektif harus sama dengan id nyata. */
        return getuid() == geteuid() && getgid() == getegid() ? 0 : 1;
    }
    if (argc == 2 && strcmp(argv[1], "sched-child") == 0) {
        struct sched_param param = {-1};
        return sched_getscheduler(0) == SCHED_OTHER &&
                       sched_getparam(0, &param) == 0 &&
                       param.sched_priority == 0
                   ? 0
                   : 1;
    }
    if (argc == 2 && strcmp(argv[1], "rt-child") == 0) {
        struct sched_param param = {-1};
        return sched_getscheduler(0) == SCHED_FIFO &&
                       sched_getparam(0, &param) == 0 &&
                       param.sched_priority == 10
                   ? 0
                   : 1;
    }
    if (argc == 2 && strcmp(argv[1], "attribute-child") == 0) {
        sigset_t current;
        if (sigprocmask(SIG_SETMASK, NULL, &current) != 0) {
            return 1;
        }
        /* POSIX_SPAWN_SETSIGMASK blocks SIGUSR1 and leaves SIGINT unblocked. */
        if (sigismember(&current, SIGUSR1) != 1 ||
            sigismember(&current, SIGINT) != 0) {
            return 2;
        }
        /* POSIX_SPAWN_SETSIGDEF resets SIGUSR1 from SIG_IGN to SIG_DFL. */
        return signal(SIGUSR1, SIG_IGN) == SIG_DFL ? 0 : 3;
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

    /* File actions: addopen redirects the child's stdout to a file. */
    posix_spawn_file_actions_t actions;
    posix_spawnattr_t attributes;
    if (posix_spawn_file_actions_init(&actions) != 0 ||
        posix_spawnattr_init(&attributes) != 0) {
        return 10;
    }
    if (posix_spawnattr_setflags(&attributes, POSIX_SPAWN_SETPGROUP) != 0 ||
        posix_spawnattr_setpgroup(&attributes, 0) != 0) {
        return 11;
    }
    if (posix_spawn_file_actions_addopen(&actions, STDOUT_FILENO, ACTION_PATH,
                                         O_WRONLY | O_CREAT | O_TRUNC,
                                         0644) != 0) {
        return 12;
    }
    char *action_arguments[] = {argv[0], "action-child", NULL};
    pid_t action_child = 0;
    error = posix_spawn(&action_child, argv[0], &actions, &attributes,
                        action_arguments, environment);
    if (error != 0 || wait_for_child(action_child) != 0) {
        return 13;
    }

    int action_fd = open(ACTION_PATH, O_RDONLY);
    if (action_fd < 0) {
        return 14;
    }
    char buffer[64];
    ssize_t count = read(action_fd, buffer, sizeof(buffer));
    close(action_fd);
    if (count != (ssize_t)(sizeof(action_message) - 1) ||
        memcmp(buffer, action_message, sizeof(action_message) - 1) != 0) {
        return 15;
    }

    /* File actions: adddup2 and addclose feed the child's stdout to a pipe. */
    int action_pipe[2];
    if (pipe(action_pipe) != 0 || posix_spawn_file_actions_init(&actions) != 0) {
        return 16;
    }
    if (posix_spawn_file_actions_adddup2(&actions, action_pipe[1],
                                         STDOUT_FILENO) != 0 ||
        posix_spawn_file_actions_addclose(&actions, action_pipe[1]) != 0) {
        return 17;
    }
    if (posix_spawnattr_init(&attributes) != 0) {
        return 18;
    }
    pid_t pipe_child = 0;
    error = posix_spawn(&pipe_child, argv[0], &actions, &attributes,
                        action_arguments, environment);
    if (error != 0) {
        return 19;
    }
    close(action_pipe[1]);
    count = read(action_pipe[0], buffer, sizeof(buffer));
    close(action_pipe[0]);
    if (count != (ssize_t)(sizeof(action_message) - 1) ||
        memcmp(buffer, action_message, sizeof(action_message) - 1) != 0 ||
        wait_for_child(pipe_child) != 0) {
        return 20;
    }

    /* Negative cases: over-long open path and unsupported attributes. */
    char long_path[OCTAGRAM_SPAWN_MAX_PATH + 2];
    memset(long_path, 'a', sizeof(long_path) - 1);
    long_path[sizeof(long_path) - 1] = '\0';
    if (posix_spawn_file_actions_init(&actions) != 0 ||
        posix_spawn_file_actions_addopen(&actions, STDOUT_FILENO, long_path,
                                         O_WRONLY, 0644) != ENAMETOOLONG) {
        return 21;
    }
    char *resetid_arguments[] = {argv[0], "resetids-child", NULL};
    if (posix_spawnattr_init(&attributes) != 0 ||
        posix_spawnattr_setflags(&attributes, POSIX_SPAWN_RESETIDS) != 0) {
        return 22;
    }
    error = posix_spawn(&child_pid, argv[0], NULL, &attributes,
                        resetid_arguments, environment);
    if (error != 0 || wait_for_child(child_pid) != 0) {
        return 23;
    }
    /* Atribut scheduling: SCHED_OTHER dengan prioritas 0 harus diterapkan. */
    struct sched_param schedparam = {0};
    if (posix_spawnattr_init(&attributes) != 0 ||
        posix_spawnattr_setflags(&attributes, POSIX_SPAWN_SETSCHEDULER |
                                                 POSIX_SPAWN_SETSCHEDPARAM) != 0 ||
        posix_spawnattr_setscheduler(&attributes, SCHED_OTHER) != 0 ||
        posix_spawnattr_setschedparam(&attributes, &schedparam) != 0) {
        return 32;
    }
    int read_policy = -1;
    struct sched_param read_param = {-1};
    if (posix_spawnattr_getscheduler(&attributes, &read_policy) != 0 ||
        read_policy != SCHED_OTHER ||
        posix_spawnattr_getschedparam(&attributes, &read_param) != 0 ||
        read_param.sched_priority != 0) {
        return 33;
    }
    char *sched_arguments[] = {argv[0], "sched-child", NULL};
    error = posix_spawn(&child_pid, argv[0], NULL, &attributes, sched_arguments,
                        environment);
    if (error != 0 || wait_for_child(child_pid) != 0) {
        return 34;
    }
    /* Kebijakan RT: root punya CAP_SCHED_NICE sehingga SCHED_FIFO prioritas 10
     * benar-benar diterapkan di anak, sedangkan prioritas 0 untuk FIFO ditolak
     * kernel dengan EINVAL. */
    struct sched_param rt_param = {10};
    if (posix_spawnattr_setscheduler(&attributes, SCHED_FIFO) != 0 ||
        posix_spawnattr_setschedparam(&attributes, &rt_param) != 0) {
        return 35;
    }
    char *rt_arguments[] = {argv[0], "rt-child", NULL};
    error = posix_spawn(&child_pid, argv[0], NULL, &attributes, rt_arguments,
                        environment);
    if (error != 0 || wait_for_child(child_pid) != 0) {
        return 36;
    }
    rt_param.sched_priority = 0;
    if (posix_spawnattr_setschedparam(&attributes, &rt_param) != 0) {
        return 37;
    }
    error = posix_spawn(&child_pid, argv[0], NULL, &attributes, rt_arguments,
                        environment);
    if (error != EINVAL) {
        return 38;
    }
    if (posix_spawn_file_actions_destroy(&actions) != 0 ||
        posix_spawnattr_destroy(&attributes) != 0) {
        return 24;
    }

    /* Attributes: SETSIGMASK and SETSIGDEF together with their getters. */
    sigset_t blocked;
    sigset_t defaulted;
    sigset_t read_back;
    short read_flags = 0;
    sigemptyset(&blocked);
    sigemptyset(&defaulted);
    if (sigaddset(&blocked, SIGUSR1) != 0 ||
        sigaddset(&defaulted, SIGUSR1) != 0) {
        return 25;
    }
    sighandler_t previous_handler = signal(SIGUSR1, SIG_IGN);
    if (previous_handler == SIG_ERR) {
        return 26;
    }
    if (posix_spawnattr_init(&attributes) != 0 ||
        posix_spawnattr_setflags(&attributes, POSIX_SPAWN_SETSIGMASK |
                                                 POSIX_SPAWN_SETSIGDEF) != 0 ||
        posix_spawnattr_setsigmask(&attributes, &blocked) != 0 ||
        posix_spawnattr_setsigdefault(&attributes, &defaulted) != 0) {
        return 27;
    }
    if (posix_spawnattr_getflags(&attributes, &read_flags) != 0 ||
        read_flags != (POSIX_SPAWN_SETSIGMASK | POSIX_SPAWN_SETSIGDEF) ||
        posix_spawnattr_getsigmask(&attributes, &read_back) != 0 ||
        read_back != blocked ||
        posix_spawnattr_getsigdefault(&attributes, &read_back) != 0 ||
        read_back != defaulted) {
        return 28;
    }
    char *attribute_arguments[] = {argv[0], "attribute-child", NULL};
    pid_t attribute_child = 0;
    error = posix_spawn(&attribute_child, argv[0], NULL, &attributes,
                        attribute_arguments, environment);
    if (signal(SIGUSR1, previous_handler) != SIG_IGN) {
        return 29;
    }
    if (error != 0 || wait_for_child(attribute_child) != 0) {
        return 30;
    }

    return printf("posix_spawn environment/status/WNOHANG/file actions/"
                  "signal attributes/scheduling/resetids passed\n") < 0
               ? 31
               : 0;
}
