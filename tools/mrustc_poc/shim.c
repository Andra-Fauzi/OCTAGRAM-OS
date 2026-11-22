/* Runtime hooks that mrustc's C backend expects outside the C library.
 * Math symbols are supplied by the bootstrap implementation in libc.a. */
#include <setjmp.h>
#include <stdint.h>

__thread jmp_buf* mrustc_panic_target;
__thread void* mrustc_panic_value;

void _Unwind_Resume(void) {
    for (;;) {
    }
}

/* mrustc emits `__rust_alloc` and friends that forward to the Rust default
 * allocator hooks (`__rdl_*`). This PoC never allocates, and an allocator that
 * hands out memory nobody can own would be worse than stopping, so every hook
 * halts with interrupts unchanged instead of returning a bogus pointer. */
int8_t* __rdl_alloc(uintptr_t size, uintptr_t align) {
    (void)size;
    (void)align;
    for (;;) {
    }
}

void __rdl_dealloc(int8_t* pointer, uintptr_t size, uintptr_t align) {
    (void)pointer;
    (void)size;
    (void)align;
    for (;;) {
    }
}

int8_t* __rdl_realloc(int8_t* pointer, uintptr_t old_size, uintptr_t align,
                      uintptr_t new_size) {
    (void)pointer;
    (void)old_size;
    (void)align;
    (void)new_size;
    for (;;) {
    }
}

int8_t* __rdl_alloc_zeroed(uintptr_t size, uintptr_t align) {
    (void)size;
    (void)align;
    for (;;) {
    }
}

void __rdl_oom(uintptr_t size, uintptr_t align) {
    (void)size;
    (void)align;
    for (;;) {
    }
}

/* mrustc's generated `panic_impl` trampoline calls this fixed mangled name for
 * the crate's `#[panic_handler]`, but the transpiler does not emit a definition
 * for it. The PoC handler never returns, so the stub halts the same way. */
uint32_t ZRG1cD3bin5panic0g(uintptr_t payload) {
    (void)payload;
    for (;;) {
    }
}
