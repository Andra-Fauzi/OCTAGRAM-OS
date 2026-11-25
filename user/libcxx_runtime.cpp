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

// `__libcpp_verbose_abort` and `__throw_bad_alloc` are both defined by
// libc++.a (verbose_abort.o, stdexcept.o). Defining them here as well makes
// every program that pulls those archive members fail to link with "multiple
// definition", so the libc++ versions are used.

// src/condition_variable.cpp pulls in src/thread.cpp (std::__thread_struct),
// promise/future support, and the libc++ support shim layer, none of which the
// OCTAGRAM port has yet. The class is a thin wrapper over pthread condition
// variables that user/pthread.h already provides, so the members needed by
// mutex.cpp and by guest code are defined here instead.
// The destructor itself lives in libc++.a (condition_variable_destructor.o);
// only the members from src/condition_variable.cpp are missing here.

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

// std::__throw_bad_alloc comes from libc++.a (stdexcept.o), which aborts via
// the verbose-abort path when built without exceptions.

// std::terminate is throw-site machinery from src/exception.cpp. Without
// unwinding the only correct action is to stop the process.
void terminate() _NOEXCEPT { abort(); }

// libc++ implements std::exception_ptr through libc++abi's
// __exceptionPtrCreate/Copy/Destroy, which this port does not have. With
// -fno-exceptions no exception object can ever be created, so the pointer is
// always null and a shallow copy is exact; rethrow_exception is unreachable
// but still has to exist for std::thread and std::future to link.
exception_ptr::exception_ptr(const exception_ptr& other) _NOEXCEPT
    : __ptr_(other.__ptr_)
{
}

exception_ptr& exception_ptr::operator=(const exception_ptr& other) _NOEXCEPT
{
    __ptr_ = other.__ptr_;
    return *this;
}

exception_ptr::~exception_ptr() _NOEXCEPT {}

exception_ptr exception_ptr::__from_native_exception_pointer(void* pointer) _NOEXCEPT
{
    exception_ptr result;
    result.__ptr_ = pointer;
    return result;
}

_LIBCPP_NORETURN void rethrow_exception(exception_ptr)
{
    abort();
}

exception_ptr current_exception() _NOEXCEPT { return exception_ptr(); }

} // namespace std
