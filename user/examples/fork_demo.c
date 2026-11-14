#include <stdio.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

int main(void)
{
    volatile int stack_value = 11;
    int *heap_value = malloc(sizeof(*heap_value));
    if (heap_value == NULL) {
        return 1;
    }
    *heap_value = 22;

    pid_t child = fork();
    if (child < 0) {
        return 2;
    }
    if (child == 0) {
        stack_value = 99;
        *heap_value = 99;
        exit(7);
    }

    int status = 0;
    if (waitpid(child, &status, 0) != child) {
        return 3;
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 7) {
        return 4;
    }
    if (stack_value != 11 || *heap_value != 22) {
        return 5;
    }
    free(heap_value);
    puts("fork returned in parent and child with private memory");
    return 0;
}
