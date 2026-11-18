#include <stdint.h>
#include <stdio.h>
#include <sys/mman.h>
#include <unistd.h>

int shared_library_message(void);

int main(void)
{
    void *mapping = mmap(NULL, 4096, PROT_READ | PROT_WRITE,
                         MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (mapping == MAP_FAILED) {
        return 1;
    }

    void *heap = sbrk(0);
    if (heap == (void *)-1) {
        return 4;
    }
    int stack_marker = 0;
    printf("aslr: main=%p library=%p mmap=%p heap=%p stack=%p\n",
           (void *)(uintptr_t)main,
           (void *)(uintptr_t)shared_library_message,
           mapping, heap, (void *)&stack_marker);

    if (munmap(mapping, 4096) != 0) {
        return 2;
    }
    if (shared_library_message() < 0) {
        return 3;
    }
    return 0;
}
