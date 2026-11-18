#include <dlfcn.h>
#include <stdio.h>

int main(void)
{
    void *handle = dlopen("libdyn_demo.so", RTLD_NOW);
    if (handle == NULL) {
        printf("dlopen failed: %s\n", dlerror());
        return 1;
    }

    int (*message)(void) = (int (*)(void))dlsym(handle, "shared_library_message");
    if (message == NULL) {
        return 2;
    }
    if (message() < 0) {
        return 3;
    }

    if (dlsym(handle, "shared_library_missing") != NULL || dlerror() == NULL) {
        return 4;
    }
    int (*global)(void) = (int (*)(void))dlsym(RTLD_DEFAULT, "shared_library_message");
    if (global == NULL || global() < 0) {
        return 5;
    }
    int (*next)(void) = (int (*)(void))dlsym(RTLD_NEXT, "shared_library_message");
    if (next == NULL || next() < 0) {
        return 6;
    }
    void *second = dlopen("libdyn_demo.so", RTLD_NOW);
    if (second != handle) {
        return 7;
    }
    if (dlclose(handle) != 0) {
        return 8;
    }
    /* Referensi masih dipegang handle kedua, jadi simbol tetap ada. */
    if (dlsym(RTLD_DEFAULT, "shared_library_message") == NULL) {
        return 9;
    }
    if (dlclose(second) != 0) {
        return 10;
    }
    if (dlsym(RTLD_DEFAULT, "shared_library_message") != NULL || dlerror() == NULL) {
        return 11;
    }

    printf("dlopen/dlsym/dlclose passed\n");
    return 0;
}
