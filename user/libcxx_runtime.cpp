// Minimal libc++ runtime support for OCTAGRAM.
//
// The vendored libc++ headers are compiled with -fno-exceptions, so the
// `__throw_*` helpers in <stdexcept> reduce to _LIBCPP_VERBOSE_ABORT, which
// calls this function. libc++'s own library would print to stderr and abort;
// until the rest of libc++ is ported this is the only symbol the headers need.
#include <__verbose_abort>
#include <condition_variable>
#include <exception>
#include <mutex>
#include <pthread.h>
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

// std::terminate is throw-site machinery from src/exception.cpp. Without
// unwinding the only correct action is to stop the process.
void terminate() _NOEXCEPT { abort(); }



// src/condition_variable.cpp pulls in src/thread.cpp (std::__thread_struct),
// promise/future support, and the libc++ support shim layer, none of which the
// OCTAGRAM port has yet. The class is a thin wrapper over pthread condition
// variables that user/pthread.h already provides, so the members needed by
// mutex.cpp and by guest code are defined here instead.
condition_variable::~condition_variable() { pthread_cond_destroy(&__cv_); }

void condition_variable::notify_one() _NOEXCEPT { pthread_cond_signal(&__cv_); }

void condition_variable::notify_all() _NOEXCEPT { pthread_cond_broadcast(&__cv_); }

void condition_variable::wait(unique_lock<mutex>& lock)
{
    pthread_cond_wait(&__cv_, lock.mutex()->native_handle());
}

_LIBCPP_END_NAMESPACE_STD

// libc++ defines the out-of-line members of std::exception in
// src/exception.cpp. That translation unit also carries the pre-C++17
// unexpected_handler API, which C++17 removed, so it is not built here. The
// members OCTAGRAM needs (the virtual destructor that every std:: exception
// subclass links against) are provided directly instead. std::exception lives
// in std, not in the libc++ inline namespace, hence the separate block. There
// is no exception unwinding in this port: with -fno-exceptions every throw
// site in libc++ aborts through __libcpp_verbose_abort above.
namespace std {

exception::~exception() _NOEXCEPT {}

const char* exception::what() const _NOEXCEPT { return "std::exception"; }

// std::uncaught_exception was removed in C++20 but libc++ 18's iostream
// sentry still calls it. Nothing can be in flight in a -fno-exceptions port.
bool uncaught_exception() _NOEXCEPT { return false; }

// libc++ routes allocation failure through std::__throw_bad_alloc; without
// exceptions it has to abort. It lives directly in std, not in the inline
// namespace.
_LIBCPP_NORETURN void __throw_bad_alloc() { abort(); }

} // namespace std
