#ifndef OCTAGRAM_DLFCN_H
#define OCTAGRAM_DLFCN_H

#define RTLD_LAZY 0x0001
#define RTLD_NOW 0x0002
#define RTLD_LOCAL 0
#define RTLD_GLOBAL 0x0100

/* Handle semu: "semua objek yang dimuat" dan "objek setelah pemanggil". */
#define RTLD_DEFAULT ((void *)0)
#define RTLD_NEXT ((void *)-1)

void *dlopen(const char *path, int flags);
void *dlsym(void *handle, const char *symbol);
int dlclose(void *handle);
char *dlerror(void);

#endif
