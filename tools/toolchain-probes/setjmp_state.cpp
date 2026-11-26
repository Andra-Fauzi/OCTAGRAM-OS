// Probe: <setjmp.h> used from C++ (as LLVM's CrashRecoveryContext does).
//
// This caught a real gap: the sysroot's jmp_buf was an unnamed struct, so the
// _setjmp/longjmp declarations were rejected by the C++ front end, and without
// C linkage they would have linked against mangled names. The probe only has
// to compile and link; the guest tests exercise the runtime behaviour.

#include <cstdio>
#include <setjmp.h>

namespace {

jmp_buf recovery_point;
int stage = 0;

void fail_later() {
    stage = 1;
    longjmp(recovery_point, 42);
}

} // namespace

int main() {
    int caught = 0;
    if (setjmp(recovery_point) == 0) {
        fail_later();
    } else {
        caught = stage;
    }
    std::printf("setjmp=%d\n", caught);
    return 0;
}
