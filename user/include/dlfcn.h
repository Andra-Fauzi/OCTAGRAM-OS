#ifndef OCTAGRAM_DLFCN_H
#define OCTAGRAM_DLFCN_H

#define RTLD_LAZY 0x0001
#define RTLD_NOW 0x0002
#define RTLD_LOCAL 0
#define RTLD_GLOBAL 0x0100

/* Handle semu: "semua objek yang dimuat" dan "objek setelah pemanggil". */
#define RTLD_DEFAULT ((void *)0)
#define RTLD_NEXT ((void *)-1)

/* Filled in by dladdr. `dli_fname` points at the loader's own copy of the
 * object name, so it stays valid as long as the object is loaded. */
typedef struct {
    const char *dli_fname;
    void *dli_fbase;
    const char *dli_sname;
    void *dli_saddr;
} Dl_info;

void *dlopen(const char *path, int flags);
void *dlsym(void *handle, const char *symbol);
int dlclose(void *handle);
char *dlerror(void);

/* Returns non-zero when `address` falls inside a loaded object; symbol names
 * are only available for dynamically linked programs (the loader owns the
 * symbol tables). A static binary returns 0, like glibc does for addresses it
 * cannot attribute. */
int dladdr(const void *address, Dl_info *info);

#endif
