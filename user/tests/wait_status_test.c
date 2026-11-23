#include <sys/wait.h>

int main(void)
{
    int exited = 7 << 8;
    int signaled = 9;
    int dumped = 11 | 0x80;
    int stopped = (19 << 8) | 0x7f;

    if (!WIFEXITED(exited) || WEXITSTATUS(exited) != 7)
        return 1;
    if (WIFSIGNALED(exited) || WIFSTOPPED(exited))
        return 2;
    if (!WIFSIGNALED(signaled) || WTERMSIG(signaled) != 9)
        return 3;
    if (!WIFSIGNALED(dumped) || WTERMSIG(dumped) != 11 || !WCOREDUMP(dumped))
        return 4;
    if (!WIFSTOPPED(stopped) || WSTOPSIG(stopped) != 19)
        return 5;
    if (!WIFCONTINUED(0xffff))
        return 6;
    return 0;
}
