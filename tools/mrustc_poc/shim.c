/* Runtime hooks that mrustc's C backend expects outside the C library.
 * Math symbols are supplied by the bootstrap implementation in libc.a. */
#include <setjmp.h>

__thread jmp_buf* mrustc_panic_target;
__thread void* mrustc_panic_value;

void _Unwind_Resume(void) {
    for (;;) {
    }
}
