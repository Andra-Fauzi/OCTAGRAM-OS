// A rustc driver image and LLVM's pass pipelines allocate tens of megabytes at
// once (instruction buffers, MIR arenas). This probe exercises the same
// allocation paths — new[], malloc and a growing std::vector — so a missing
// allocator entry point or an over-aligned allocation shows up here rather than
// during the LLVM link.
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <vector>

int main()
{
    const size_t big = (size_t)64 << 20;
    char *block = new char[big];
    for (size_t offset = 0; offset < big; offset += 4096) {
        block[offset] = (char)(offset >> 12);
    }
    int ok = block[0] == 0 && block[big - 4096] == (char)((big - 4096) >> 12);
    delete[] block;

    char *raw = static_cast<char *>(malloc((size_t)8 << 20));
    if (raw == NULL) {
        return 1;
    }
    memset(raw, 7, (size_t)8 << 20);
    ok = ok && raw[0] == 7;
    free(raw);

    std::vector<char> grown;
    grown.resize((size_t)32 << 20);
    grown.back() = 1;
    ok = ok && grown.back() == 1;

    return ok ? 0 : 2;
}
