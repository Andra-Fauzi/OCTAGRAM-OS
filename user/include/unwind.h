#ifndef OCTAGRAM_UNWIND_H
#define OCTAGRAM_UNWIND_H

/* Minimal Itanium-ABI unwinder surface.
 *
 * OCTAGRAM has no exception support (C++ is built with -fno-exceptions), so
 * this header exists for the introspection entry points that <execinfo.h>,
 * libc++'s <stacktrace> and LLVM's PrintStackTrace need. Only the frame walker
 * is implemented; the phase-based _Unwind_RaiseException family is not, and
 * calling it would link-fail on purpose rather than pretend to unwind. */

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    _URC_NO_REASON = 0,
    _URC_FOREIGN_EXCEPTION_CAUGHT = 1,
    _URC_FATAL_PHASE2_ERROR = 2,
    _URC_FATAL_PHASE1_ERROR = 3,
    _URC_NORMAL_STOP = 4,
    _URC_END_OF_STACK = 5,
    _URC_HANDLER_FOUND = 6,
    _URC_INSTALL_CONTEXT = 7,
    _URC_CONTINUE_UNWIND = 8
} _Unwind_Reason_Code;

struct _Unwind_Context;

typedef _Unwind_Reason_Code (*_Unwind_Trace_Fn)(struct _Unwind_Context *,
                                                void *);

/* Walks the caller's frame chain and calls `trace` for every return address,
 * stopping when `trace` returns anything other than _URC_NO_REASON. */
_Unwind_Reason_Code _Unwind_Backtrace(_Unwind_Trace_Fn trace, void *argument);

uintptr_t _Unwind_GetIP(struct _Unwind_Context *context);
uintptr_t _Unwind_GetIPInfo(struct _Unwind_Context *context, int *before_insn);
uintptr_t _Unwind_GetCFA(struct _Unwind_Context *context);

#ifdef __cplusplus
}
#endif
#endif
