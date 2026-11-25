/* Minimal C++ ABI support for OCTAGRAM userspace (M4.3.2).
 *
 * This is not a libc++/libstdc++ port: it only supplies the symbols the
 * compiler emits for a freestanding C++ program that uses operator new/delete,
 * virtual dispatch, and function-local statics. Exceptions, RTTI, and the C++
 * standard library itself stay out of scope until their runtime dependencies
 * exist.
 */
#include <stddef.h>
#include <stdlib.h>

/* Referenced by GCC's __cxa_atexit calls emitted for static initializers.
 * A null handle means "never dlclose", which matches a static executable. */
extern "C" {
void *__dso_handle = nullptr;
}

void *operator new(size_t size)
{
    void *memory = malloc(size == 0 ? 1 : size);
    if (memory == nullptr) {
        abort();
    }
    return memory;
}

void *operator new[](size_t size)
{
    return ::operator new(size);
}

void operator delete(void *memory) noexcept
{
    free(memory);
}

void operator delete[](void *memory) noexcept
{
    free(memory);
}

void operator delete(void *memory, size_t) noexcept
{
    free(memory);
}

void operator delete[](void *memory, size_t) noexcept
{
    free(memory);
}

/* C++17 over-aligned allocation. libc++'s own src/new.cpp is not part of the
 * userspace libc++ archive, so these have to live here; without them any
 * `alignas(32)` type, and libc++ 18's own string/vector fast paths, fail to
 * link against libc++.a.
 *
 * `<new>` is not on the freestanding include path used for this file, so the
 * scoped enum is forward-declared with its real underlying type. That is
 * enough to produce the mangled names libc++ and the compiler expect
 * (`_ZnwmSt11align_val_t`, `_ZdlPvmSt11align_val_t`, ...). */
namespace std {
enum class align_val_t : size_t;
}

static void *allocate_over_aligned(size_t size, size_t alignment)
{
    if (size == 0) {
        size = 1;
    }
    if (alignment <= sizeof(void *)) {
        return ::operator new(size);
    }
    void *memory = nullptr;
    if (posix_memalign(&memory, alignment, size) != 0) {
        memory = nullptr;
    }
    if (memory == nullptr) {
        abort();
    }
    return memory;
}

void *operator new(size_t size, std::align_val_t alignment)
{
    return allocate_over_aligned(size, static_cast<size_t>(alignment));
}

void *operator new[](size_t size, std::align_val_t alignment)
{
    return allocate_over_aligned(size, static_cast<size_t>(alignment));
}

void operator delete(void *memory, std::align_val_t) noexcept
{
    free(memory);
}

void operator delete[](void *memory, std::align_val_t) noexcept
{
    free(memory);
}

void operator delete(void *memory, size_t, std::align_val_t) noexcept
{
    free(memory);
}

void operator delete[](void *memory, size_t, std::align_val_t) noexcept
{
    free(memory);
}

/* Called when a pure virtual function is dispatched on an abstract base. */
extern "C" void __cxa_pure_virtual()
{
    abort();
}

/* Single-threaded guard for function-local statics. OCTAGRAM userspace builds
 * currently pass -fno-threadsafe-statics, so these are only a safety net for
 * translation units compiled without that flag. */
extern "C" int __cxa_guard_acquire(long long *guard)
{
    return *guard == 0;
}

extern "C" void __cxa_guard_release(long long *guard)
{
    *guard = 1;
}

extern "C" void __cxa_guard_abort(long long *)
{
}
