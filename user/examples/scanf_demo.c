#include <stdio.h>
#include <string.h>

int main(void)
{
    int count = 0;
    int value = 0;
    char name[16];
    if (scanf("%15s %d", name, &value) != 2) {
        return 1;
    }
    if (sscanf("42 items", "%d %15s%n", &count, name, &value) != 2 ||
        count != 42 || strcmp(name, "items") != 0 || value != 8) {
        return 2;
    }
    printf("scanf passed: %s %d\n", name, count);
    return 0;
}
