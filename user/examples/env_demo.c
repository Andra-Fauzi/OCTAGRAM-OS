#include <errno.h>
#include <spawn.h>
#include <stdlib.h>
#include <stdio.h>
#include <sys/wait.h>
#include <unistd.h>

static int equals(const char *left, const char *right)
{
    while (*left != '\0' && *left == *right) {
        ++left;
        ++right;
    }
    return *left == *right;
}

static int starts_with(const char *text, const char *prefix)
{
    while (*prefix != '\0' && *text == *prefix) {
        ++text;
        ++prefix;
    }
    return *prefix == '\0';
}

static int has_expected_environment(char **envp)
{
    if (envp == 0 || environ != envp) {
        return 0;
    }
    char *test_value = getenv("testvar");
    if (test_value == 0 || !equals(test_value, "passed")) {
        return 0;
    }
    if (getenv("testvar_extra") != 0 || getenv("") != 0) {
        return 0;
    }

    int found_path = 0;
    int found_test_value = 0;
    for (char **entry = envp; *entry != 0; ++entry) {
        found_path |= starts_with(*entry, "PATH=");
        found_test_value |= equals(*entry, "testvar=passed");
    }
    return found_path && found_test_value;
}

static int has_updated_environment(char **envp)
{
    if (envp == 0 || environ != envp) {
        return 0;
    }
    char *value = getenv("OCTAGRAM_TEST");
    if (value == 0 || !equals(value, "parent") || getenv("testvar") != 0) {
        return 0;
    }

    int found_path = 0;
    int found_updated_value = 0;
    for (char **entry = envp; *entry != 0; ++entry) {
        found_path |= starts_with(*entry, "PATH=");
        found_updated_value |= equals(*entry, "OCTAGRAM_TEST=parent");
    }
    return found_path && found_updated_value;
}

int main(int argc, char **argv, char **envp)
{
    if (argc == 2 && equals(argv[1], "child")) {
        return has_updated_environment(envp) ? 0 : 6;
    }
    if (argc != 1) {
        return 1;
    }
    if (!has_expected_environment(envp)) {
        return 2;
    }
    if (setenv("OCTAGRAM_TEST", "parent", 1) != 0 ||
        setenv("OCTAGRAM_TEST", "ignored", 0) != 0) {
        return 6;
    }
    char *updated_value = getenv("OCTAGRAM_TEST");
    if (updated_value == 0 || !equals(updated_value, "parent")) {
        return 7;
    }
    if (setenv("BAD=NAME", "invalid", 1) != -1 || errno != EINVAL) {
        return 8;
    }
    if (unsetenv("testvar") != 0 || !has_updated_environment(environ)) {
        return 9;
    }

    char *child_arguments[] = {argv[0], "child", NULL};
    pid_t child_pid;
    int error = posix_spawn(&child_pid, argv[0], NULL, NULL,
                            child_arguments, environ);
    if (error != 0) {
        return 10;
    }
    int child_status = 0;
    if (waitpid(child_pid, &child_status, 0) != child_pid ||
        !WIFEXITED(child_status) || WEXITSTATUS(child_status) != 0) {
        return 11;
    }

    return puts("environment passed") < 0 ? 12 : 0;
}
