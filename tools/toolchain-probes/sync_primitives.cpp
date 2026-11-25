// libLLVM and rustc keep their thread pools, caches and diagnostics behind
// std::mutex, std::condition_variable and std::once_flag. Linking this probe
// answers "can a threaded C++ program build against this runtime?" without
// waiting for the real LLVM build, and it never blocks.
#include <condition_variable>
#include <mutex>

static std::mutex global_mutex;
static std::condition_variable global_condition;
static std::once_flag once;
static int initialised;

static void initialise()
{
    std::lock_guard<std::mutex> guard(global_mutex);
    initialised++;
}

int main()
{
    std::call_once(once, initialise);
    {
        std::unique_lock<std::mutex> lock(global_mutex);
        global_condition.notify_one();
        global_condition.notify_all();
        lock.unlock();
    }

    // Taking the address forces libc++'s out-of-line wait() member into the
    // link; the probe never calls it, so nothing blocks.
    void (std::condition_variable::*wait_member)(std::unique_lock<std::mutex> &) =
        &std::condition_variable::wait;
    (void)wait_member;

    if (global_mutex.try_lock()) {
        global_mutex.unlock();
    }
    return initialised == 1 ? 0 : 1;
}
