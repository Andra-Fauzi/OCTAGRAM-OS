// Minimal libc++ runtime support for OCTAGRAM.
//
// The vendored libc++ headers are compiled with -fno-exceptions, so the
// `__throw_*` helpers in <stdexcept> reduce to _LIBCPP_VERBOSE_ABORT, which
// calls this function. libc++'s own library would print to stderr and abort;
// until the rest of libc++ is ported this is the only symbol the headers need.
#include <__verbose_abort>
#include <stdarg.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

_LIBCPP_BEGIN_NAMESPACE_STD

void __libcpp_verbose_abort(const char* format, ...)
{
    char buffer[256];
    va_list arguments;
    va_start(arguments, format);
    int length = vsnprintf(buffer, sizeof buffer, format, arguments);
    va_end(arguments);
    if (length < 0) {
        length = 0;
    }
    if ((size_t)length >= sizeof buffer) {
        length = (int)(sizeof buffer - 1);
    }
    static const char prefix[] = "libc++ abort: ";
    (void)!write(2, prefix, sizeof prefix - 1);
    (void)!write(2, buffer, (size_t)length);
    (void)!write(2, "\n", 1);
    abort();
}

_LIBCPP_END_NAMESPACE_STD
