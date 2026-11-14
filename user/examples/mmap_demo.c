#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <unistd.h>

static int fail(const char *message)
{
    write(STDERR_FILENO, message, strlen(message));
    return 1;
}

int main(void)
{
    if (mlockall(MCL_CURRENT | MCL_FUTURE) != 0) {
        return fail("mlockall failed\n");
    }

    unsigned char *anonymous = mmap(NULL, 8192, PROT_READ | PROT_WRITE,
                                    MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (anonymous == MAP_FAILED || anonymous[0] != 0) {
        return fail("mmap anonymous mapping failed\n");
    }
    anonymous[0] = 0x31;

    unsigned char residency[2] = {0, 0};
    if (mincore(anonymous, 8192, residency) != 0
        || (residency[0] & 1) == 0 || (residency[1] & 1) == 0) {
        return fail("mincore failed\n");
    }
    if (mprotect(anonymous, 4096, PROT_READ) != 0
        || mprotect(anonymous, 4096, PROT_READ | PROT_WRITE) != 0) {
        return fail("mprotect failed\n");
    }
    if (mlock(anonymous, 4096) != 0
        || munlock(anonymous, 4096) != 0
        || posix_madvise(anonymous, 8192, POSIX_MADV_SEQUENTIAL) != 0) {
        return fail("mapping control operation failed\n");
    }

    unsigned char *shared_fork = mmap(NULL, 4096, PROT_READ | PROT_WRITE,
                                      MAP_SHARED | MAP_ANONYMOUS, -1, 0);
    unsigned char *private_fork = mmap(NULL, 4096, PROT_READ | PROT_WRITE,
                                       MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (shared_fork == MAP_FAILED || private_fork == MAP_FAILED) {
        return fail("fork mapping setup failed\n");
    }
    shared_fork[0] = 1;
    private_fork[0] = 1;
    pid_t child = fork();
    if (child < 0) {
        return fail("fork with mappings failed\n");
    }
    if (child == 0) {
        shared_fork[0] = 2;
        private_fork[0] = 2;
        exit(0);
    }
    int status = 0;
    if (waitpid(child, &status, 0) != child || !WIFEXITED(status)
        || WEXITSTATUS(status) != 0
        || shared_fork[0] != 2 || private_fork[0] != 1) {
        return fail("fork MAP_SHARED/MAP_PRIVATE semantics failed\n");
    }
    if (munmap(shared_fork, 4096) != 0 || munmap(private_fork, 4096) != 0) {
        return fail("fork mapping cleanup failed\n");
    }

    unsigned char *replacement = mmap(anonymous, 4096, PROT_READ | PROT_WRITE,
                                      MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED,
                                      -1, 0);
    if (replacement != anonymous || replacement[0] != 0) {
        return fail("MAP_FIXED replacement failed\n");
    }
    if (munmap(anonymous, 8192) != 0) {
        return fail("munmap anonymous mapping failed\n");
    }

    int descriptor = open("/tmp/mmap-demo", O_CREAT | O_RDWR | O_TRUNC, 0600);
    if (descriptor < 0 || write(descriptor, "file", 4) != 4) {
        return fail("mmap test file setup failed\n");
    }
    unsigned char *shared = mmap(NULL, 4096, PROT_READ | PROT_WRITE,
                                 MAP_SHARED, descriptor, 0);
    unsigned char *shared_alias = mmap(NULL, 4096, PROT_READ | PROT_WRITE,
                                       MAP_SHARED, descriptor, 0);
    if (shared == MAP_FAILED || shared_alias == MAP_FAILED) {
        return fail("file-backed MAP_SHARED failed\n");
    }
    shared[0] = 'S';
    if (shared_alias[0] != 'S' || msync(shared, 4096, MS_SYNC) != 0) {
        return fail("MAP_SHARED visibility or msync failed\n");
    }

    unsigned char *private = mmap(NULL, 4096, PROT_READ | PROT_WRITE,
                                  MAP_PRIVATE, descriptor, 0);
    if (private == MAP_FAILED) {
        return fail("file-backed MAP_PRIVATE failed\n");
    }
    private[0] = 'P';
    if (msync(private, 4096, MS_SYNC) != 0) {
        return fail("msync on private mapping failed\n");
    }
    if (lseek(descriptor, 0, SEEK_SET) < 0) {
        return fail("lseek failed\n");
    }
    char file_bytes[4];
    if (read(descriptor, file_bytes, sizeof(file_bytes)) != sizeof(file_bytes)
        || file_bytes[0] != 'S') {
        return fail("MAP_PRIVATE changed the file or shared changes were lost\n");
    }
    if (munmap(shared, 4096) != 0
        || munmap(shared_alias, 4096) != 0
        || munmap(private, 4096) != 0
        || close(descriptor) != 0
        || unlink("/tmp/mmap-demo") != 0) {
        return fail("file mapping cleanup failed\n");
    }

    if (munlockall() != 0) {
        return fail("munlockall failed\n");
    }
    write(STDOUT_FILENO, "mmap POSIX demo: PASS\n", 22);
    return 0;
}
