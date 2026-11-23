#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

#define SH_LINE_MAX 4096
#define SH_TOKEN_MAX 256
#define SH_STAGE_MAX 32
#define SH_ARG_MAX 64

typedef struct {
    char *argv[SH_ARG_MAX];
    int argc;
    char *input;
    char *output;
    char *error;
    int append;
    int error_to_output;
    int output_to_error;
} sh_stage;

static int last_status;
static int exit_requested;
static int requested_exit_status;

static int is_operator_char(char c)
{
    return c == ';' || c == '|' || c == '&' || c == '<' || c == '>';
}

static int append_text(char *out, size_t *used, const char *text)
{
    while (*text) {
        if (*used + 1 >= SH_LINE_MAX)
            return -1;
        out[(*used)++] = *text++;
    }
    return 0;
}

static int expand_variable(const char *line, size_t *position,
                           char *out, size_t *used)
{
    size_t i = *position + 1;
    char name[128];
    size_t n = 0;
    const char *value = "";
    char status_text[16];

    if (line[i] == '?') {
        snprintf(status_text, sizeof(status_text), "%d", last_status);
        *position = i + 1;
        return append_text(out, used, status_text);
    }
    if (line[i] == '{') {
        ++i;
        while (line[i] && line[i] != '}' && n + 1 < sizeof(name))
            name[n++] = line[i++];
        if (line[i] != '}')
            return -1;
        *position = i + 1;
    } else {
        while ((line[i] == '_' ||
                (line[i] >= 'a' && line[i] <= 'z') ||
                (line[i] >= 'A' && line[i] <= 'Z') ||
                (line[i] >= '0' && line[i] <= '9')) &&
               n + 1 < sizeof(name))
            name[n++] = line[i++];
        *position = i;
    }
    if (n == 0) {
        out[(*used)++] = '$';
        return 0;
    }
    name[n] = '\0';
    value = getenv(name);
    return append_text(out, used, value ? value : "");
}

static int tokenize(const char *line, char *storage, char **tokens, int capacity)
{
    static char *operators[] = {
        ";", "|", "||", "&&", "<", ">", ">>", "2>", "2>&1"
    };
    size_t used = 0;
    size_t i = 0;
    int count = 0;

    while (line[i]) {
        while (line[i] == ' ' || line[i] == '\t' ||
               line[i] == '\r' || line[i] == '\n')
            ++i;
        if (!line[i])
            break;
        if (count >= capacity - 1)
            return -1;

        if ((line[i] == '2' && line[i + 1] == '>') ||
            (line[i] == '>' && line[i + 1] == '&' && line[i + 2] == '2') ||
            is_operator_char(line[i])) {
            int length = 1;
            char *op = NULL;
            if (line[i] == '>' && line[i + 1] == '&' && line[i + 2] == '2') {
                op = operators[9];
                length = 3;
            } else if (line[i] == '2' && line[i + 1] == '>') {
                if (line[i + 2] == '&' && line[i + 3] == '1') {
                    op = operators[8];
                    length = 4;
                } else {
                    op = operators[7];
                    length = 2;
                }
            } else if (line[i] == '&' && line[i + 1] == '&') {
                op = operators[3]; length = 2;
            } else if (line[i] == '|' && line[i + 1] == '|') {
                op = operators[2]; length = 2;
            } else if (line[i] == '>' && line[i + 1] == '>') {
                op = operators[6]; length = 2;
            } else {
                switch (line[i]) {
                case ';': op = operators[0]; break;
                case '|': op = operators[1]; break;
                case '<': op = operators[4]; break;
                case '>': op = operators[5]; break;
                default: return -1;
                }
            }
            tokens[count++] = op;
            i += (size_t)length;
            continue;
        }

        size_t start = used;
        int started = 0;
        char quote = 0;
        while (line[i]) {
            char c = line[i];
            if (!quote && (c == ' ' || c == '\t' || c == '\r' ||
                           c == '\n' || is_operator_char(c) ||
                           (c == '2' && line[i + 1] == '>')))
                break;
            if (c == '\\' && quote != '\'') {
                ++i;
                if (!line[i])
                    return -1;
                if (used + 1 >= SH_LINE_MAX)
                    return -1;
                storage[used++] = line[i++];
                started = 1;
                continue;
            }
            if (c == '\'' || c == '"') {
                if (!quote) {
                    quote = c;
                    started = 1;
                    ++i;
                    continue;
                }
                if (quote == c) {
                    quote = 0;
                    ++i;
                    continue;
                }
            }
            if (c == '$' && quote != '\'') {
                if (expand_variable(line, &i, storage, &used) != 0)
                    return -1;
                started = 1;
                continue;
            }
            if (used + 1 >= SH_LINE_MAX)
                return -1;
            storage[used++] = c;
            started = 1;
            ++i;
        }
        if (quote)
            return -1;
        if (!started)
            return -1;
        storage[used++] = '\0';
        tokens[count++] = &storage[start];
    }
    tokens[count] = NULL;
    return count;
}

static int is_builtin(const char *name)
{
    return !strcmp(name, ":") || !strcmp(name, "true") ||
           !strcmp(name, "false") || !strcmp(name, "echo") ||
           !strcmp(name, "pwd") || !strcmp(name, "cat") || !strcmp(name, "cd") ||
           !strcmp(name, "export") || !strcmp(name, "unset") ||
           !strcmp(name, "exit") || !strcmp(name, "exec");
}

static int run_builtin(char **argv)
{
    const char *name = argv[0];
    if (!strcmp(name, ":") || !strcmp(name, "true"))
        return 0;
    if (!strcmp(name, "false"))
        return 1;
    if (!strcmp(name, "echo")) {
        int i = 1, newline = 1;
        if (argv[i] && !strcmp(argv[i], "-n")) {
            newline = 0;
            ++i;
        }
        for (; argv[i]; ++i) {
            if (i > (newline ? 1 : 2))
                putchar(' ');
            fputs(argv[i], stdout);
        }
        if (newline)
            putchar('\n');
        return 0;
    }
    if (!strcmp(name, "pwd")) {
        char path[1024];
        if (!getcwd(path, sizeof(path))) {
            perror("pwd");
            return 1;
        }
        puts(path);
        return 0;
    }
    if (!strcmp(name, "cat")) {
        char buffer[1024];
        int first = 1;
        do {
            int fd = argv[1] ? open(argv[first], O_RDONLY) : STDIN_FILENO;
            if (fd < 0) {
                perror(argv[first]);
                return 1;
            }
            for (;;) {
                ssize_t bytes = read(fd, buffer, sizeof(buffer));
                if (bytes == 0)
                    break;
                if (bytes < 0) {
                    perror("cat");
                    if (fd != STDIN_FILENO) close(fd);
                    return 1;
                }
                ssize_t offset = 0;
                while (offset < bytes) {
                    ssize_t written = write(STDOUT_FILENO, buffer + offset,
                                            (size_t)(bytes - offset));
                    if (written <= 0) {
                        perror("cat");
                        if (fd != STDIN_FILENO) close(fd);
                        return 1;
                    }
                    offset += written;
                }
            }
            if (fd != STDIN_FILENO)
                close(fd);
            if (!argv[1])
                break;
            ++first;
        } while (argv[first]);
        return 0;
    }
    if (!strcmp(name, "cd")) {
        const char *path = argv[1] ? argv[1] : getenv("HOME");
        if (!path)
            path = "/";
        if (argv[2]) {
            fputs("sh: cd: too many arguments\n", stderr);
            return 2;
        }
        if (chdir(path) != 0) {
            perror("cd");
            return 1;
        }
        return 0;
    }
    if (!strcmp(name, "export")) {
        for (int i = 1; argv[i]; ++i) {
            char *equal = strchr(argv[i], '=');
            if (!equal) {
                const char *value = getenv(argv[i]);
                if (value)
                    printf("%s=%s\n", argv[i], value);
                continue;
            }
            *equal = '\0';
            if (setenv(argv[i], equal + 1, 1) != 0) {
                perror("export");
                return 1;
            }
            *equal = '=';
        }
        return 0;
    }
    if (!strcmp(name, "unset")) {
        for (int i = 1; argv[i]; ++i)
            if (unsetenv(argv[i]) != 0) {
                perror("unset");
                return 1;
            }
        return 0;
    }
    if (!strcmp(name, "exit")) {
        int status = argv[1] ? atoi(argv[1]) : last_status;
        exit_requested = 1;
        requested_exit_status = status & 255;
        return requested_exit_status;
    }
    if (!strcmp(name, "exec")) {
        if (!argv[1])
            return 0;
        execvp(argv[1], &argv[1]);
        perror(argv[1]);
        return 127;
    }
    return 127;
}

static int wait_for(pid_t pid)
{
    int status = 0;
    while (waitpid(pid, &status, 0) < 0) {
        if (errno == EINTR)
            continue;
        perror("waitpid");
        return 125;
    }
    if (WIFEXITED(status))
        return WEXITSTATUS(status);
    if (WIFSIGNALED(status))
        return 128 + WTERMSIG(status);
    return 125;
}

static int apply_redirections(const sh_stage *stage)
{
    int fd;
    if (stage->input) {
        fd = open(stage->input, O_RDONLY);
        if (fd < 0 || dup2(fd, STDIN_FILENO) < 0) {
            perror(stage->input);
            if (fd >= 0) close(fd);
            return -1;
        }
        close(fd);
    }
    if (stage->output) {
        int flags = O_WRONLY | O_CREAT |
                    (stage->append ? O_APPEND : O_TRUNC);
        fd = open(stage->output, flags, 0666);
        if (fd < 0 || dup2(fd, STDOUT_FILENO) < 0) {
            perror(stage->output);
            if (fd >= 0) close(fd);
            return -1;
        }
        close(fd);
    }
    if (stage->output_to_error) {
        if (dup2(STDERR_FILENO, STDOUT_FILENO) < 0)
            return -1;
    }
    if (stage->error_to_output) {
        if (dup2(STDOUT_FILENO, STDERR_FILENO) < 0)
            return -1;
    } else if (stage->error) {
        fd = open(stage->error, O_WRONLY | O_CREAT | O_TRUNC, 0666);
        if (fd < 0 || dup2(fd, STDERR_FILENO) < 0) {
            perror(stage->error);
            if (fd >= 0) close(fd);
            return -1;
        }
        close(fd);
    }
    return 0;
}

static int run_pipeline(sh_stage *stages, int count)
{
    int previous = -1;
    pid_t children[SH_STAGE_MAX];

    if (count == 1 && !stages[0].input && !stages[0].output &&
        !stages[0].error && !stages[0].error_to_output &&
        stages[0].argc && is_builtin(stages[0].argv[0])) {
        int status = run_builtin(stages[0].argv);
        fflush(stdout);
        fflush(stderr);
        return status;
    }

    for (int i = 0; i < count; ++i) {
        int next_pipe[2] = {-1, -1};
        if (i + 1 < count && pipe(next_pipe) != 0) {
            perror("pipe");
            if (previous >= 0) close(previous);
            return 125;
        }
        pid_t pid = fork();
        if (pid < 0) {
            perror("fork");
            if (previous >= 0) close(previous);
            if (next_pipe[0] >= 0) close(next_pipe[0]);
            if (next_pipe[1] >= 0) close(next_pipe[1]);
            return 125;
        }
        if (pid == 0) {
            if (previous >= 0) {
                dup2(previous, STDIN_FILENO);
                close(previous);
            }
            if (next_pipe[1] >= 0) {
                dup2(next_pipe[1], STDOUT_FILENO);
                close(next_pipe[1]);
                close(next_pipe[0]);
            }
            if (apply_redirections(&stages[i]) != 0)
                _exit(125);
            if (!stages[i].argc)
                _exit(0);
            if (is_builtin(stages[i].argv[0])) {
                int status = run_builtin(stages[i].argv);
                fflush(stdout);
                fflush(stderr);
                _exit(status);
            }
            execvp(stages[i].argv[0], stages[i].argv);
            perror(stages[i].argv[0]);
            _exit(127);
        }
        children[i] = pid;
        if (previous >= 0)
            close(previous);
        if (next_pipe[1] >= 0)
            close(next_pipe[1]);
        previous = next_pipe[0];
    }
    if (previous >= 0)
        close(previous);

    int result = 0;
    for (int i = 0; i < count; ++i) {
        int status = wait_for(children[i]);
        if (i == count - 1)
            result = status;
    }
    return result;
}

static int parse_pipeline(char **tokens, int begin, int end)
{
    sh_stage stages[SH_STAGE_MAX];
    memset(stages, 0, sizeof(stages));
    int count = 0;
    int i = begin;

    while (i < end) {
        if (count >= SH_STAGE_MAX)
            return 2;
        sh_stage *stage = &stages[count++];
        while (i < end && strcmp(tokens[i], "|") != 0) {
            char *token = tokens[i++];
            if (!strcmp(token, "<") || !strcmp(token, ">") ||
                !strcmp(token, ">>") || !strcmp(token, "2>")) {
                if (i >= end)
                    return 2;
                char *target = tokens[i++];
                if (!strcmp(token, "<"))
                    stage->input = target;
                else if (!strcmp(token, "2>"))
                    stage->error = target;
                else {
                    stage->output = target;
                    stage->append = !strcmp(token, ">>");
                }
            } else if (!strcmp(token, "2>&1")) {
                stage->error_to_output = 1;
            } else if (!strcmp(token, ">&2")) {
                stage->output_to_error = 1;
            } else {
                if (stage->argc >= SH_ARG_MAX - 1)
                    return 2;
                stage->argv[stage->argc++] = token;
            }
        }
        if (stage->argc)
            stage->argv[stage->argc] = NULL;
        if (i < end && !strcmp(tokens[i], "|"))
            ++i;
    }
    if (count == 0)
        return 0;
    return run_pipeline(stages, count);
}

static int run_command_string(const char *command)
{
    char storage[SH_LINE_MAX];
    char *tokens[SH_TOKEN_MAX];
    int count = tokenize(command, storage, tokens, SH_TOKEN_MAX);
    if (count < 0) {
        fputs("sh: syntax error or command too long\n", stderr);
        return 2;
    }

    int position = 0;
    int status = 0;
    char *connector = ";";
    while (position < count && !exit_requested) {
        int end = position;
        while (end < count && strcmp(tokens[end], ";") &&
               strcmp(tokens[end], "&&") && strcmp(tokens[end], "||"))
            ++end;
        int should_run = !strcmp(connector, ";") ||
                         (!strcmp(connector, "&&") && status == 0) ||
                         (!strcmp(connector, "||") && status != 0);
        if (should_run)
            status = parse_pipeline(tokens, position, end);
        if (end == count)
            break;
        connector = tokens[end];
        position = end + 1;
    }
    last_status = status;
    return status;
}

int main(int argc, char **argv)
{
    if (argc >= 3 && !strcmp(argv[1], "-c")) {
        int status = run_command_string(argv[2]);
        return exit_requested ? requested_exit_status : status;
    }
    if (argc > 1 && strcmp(argv[1], "-i") != 0) {
        fprintf(stderr, "usage: sh [-i] [-c command]\n");
        return 2;
    }

    char line[SH_LINE_MAX];
    int interactive = isatty(STDIN_FILENO);
    while (!exit_requested) {
        if (interactive) {
            fputs("sh$ ", stdout);
            fflush(stdout);
        }
        if (!fgets(line, sizeof(line), stdin))
            break;
        last_status = run_command_string(line);
    }
    return exit_requested ? requested_exit_status : last_status;
}
