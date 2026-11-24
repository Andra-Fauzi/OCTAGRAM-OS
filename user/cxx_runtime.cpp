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
