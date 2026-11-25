// LLVM reads object files and bitcode through MemoryBuffer, which mmaps the
// file and protects the mapping; rustc reads its query cache the same way.
// This probe performs that sequence on a real file, so a missing mmap, fstat,
// mprotect or file primitive shows up as a link error instead of a failure
// halfway through the LLVM build.
#include <fcntl.h>
#include <stddef.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

static const char probe_path[] = "/tmp/octagram-mmap-probe.bin";
static const char payload[] = "bitcode";

int main()
{
    int descriptor = open(probe_path, O_CREAT | O_TRUNC | O_RDWR, 0644);
    if (descriptor < 0) {
        return 1;
    }
    if (write(descriptor, payload, sizeof payload - 1) != (long)(sizeof payload - 1)) {
        return 2;
    }

    struct stat info;
    if (fstat(descriptor, &info) != 0) {
        return 3;
    }
    size_t size = (size_t)info.st_size;
    if (size != sizeof payload - 1) {
        return 4;
    }

    void *mapping = mmap(NULL, size, PROT_READ, MAP_PRIVATE, descriptor, 0);
    if (mapping == MAP_FAILED) {
        return 5;
    }
    int ok = memcmp(mapping, payload, size) == 0;
    if (mprotect(mapping, size, PROT_READ | PROT_WRITE) != 0) {
        ok = 0;
    }
    munmap(mapping, size);
    close(descriptor);
    unlink(probe_path);

    return (ok && getpagesize() > 0) ? 0 : 6;
}
