// rustc and LLVM keep per-thread state (diagnostic buffers, arena pointers,
// target caches) in thread_local objects with destructors. Such a variable
// makes the compiler emit a __cxa_thread_atexit call, which is a different
// runtime dependency from plain TLS.
struct ThreadState {
    ~ThreadState() { destroyed++; }
    int counter = 0;
    static int destroyed;
};

int ThreadState::destroyed = 0;

thread_local ThreadState state;
thread_local unsigned char scratch[4096];

int main()
{
    state.counter = 3;
    scratch[0] = 1;
    scratch[sizeof scratch - 1] = 2;
    int ok = state.counter == 3 && scratch[0] == 1 && scratch[sizeof scratch - 1] == 2;
    return ok ? 0 : 1;
}
