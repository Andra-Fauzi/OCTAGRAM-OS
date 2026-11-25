// Third libc++ guest probe: threading, futures, and file streams.
//
// src/thread.cpp, src/future.cpp, and src/fstream.cpp complete the part of
// libc++ that Cargo and rustc exercise most: worker threads, scoped locks,
// condition variables, promises, and reading/writing files through iostream.
//
// std::printf is used for markers so a partial failure names its stage.

#include <atomic>
#include <cstdio>
#include <fstream>
#include <future>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

namespace {

constexpr int kWorkers = 4;
constexpr int kIterations = 250;

int g_counter = 0;
std::mutex g_counter_mutex;
std::condition_variable g_ready;
bool g_start = false;

thread_local int g_thread_value = 0;

} // namespace

int main() {
    // std::thread + std::mutex + a condition variable handshake.
    std::vector<std::thread> workers;
    for (int index = 0; index < kWorkers; ++index) {
        workers.emplace_back([index]() {
            g_thread_value = index + 1;
            {
                std::unique_lock<std::mutex> lock(g_counter_mutex);
                g_ready.wait(lock, [] { return g_start; });
            }
            for (int step = 0; step < kIterations; ++step) {
                std::lock_guard<std::mutex> guard(g_counter_mutex);
                g_counter += 1;
            }
        });
    }
    {
        std::lock_guard<std::mutex> guard(g_counter_mutex);
        g_start = true;
    }
    g_ready.notify_all();
    for (std::thread& worker : workers) {
        worker.join();
    }
    if (g_counter != kWorkers * kIterations) {
        std::printf("LIBCXX-THREAD-FAIL: lost increments (%d)\n", g_counter);
        return 1;
    }
    std::printf("LIBCXX-THREAD-PASS\n");

    // thread_local is per thread, and the main thread's copy stays untouched.
    if (g_thread_value != 0) {
        std::printf("LIBCXX-THREAD-FAIL: thread_local leaked (%d)\n", g_thread_value);
        return 1;
    }
    std::printf("LIBCXX-THREAD-LOCAL-PASS\n");

    // std::promise/std::future: the value crosses threads through shared state.
    std::promise<std::string> promise;
    std::future<std::string> future = promise.get_future();
    std::thread producer([&promise]() { promise.set_value("octagram-future"); });
    std::string delivered = future.get();
    producer.join();
    if (delivered != "octagram-future") {
        std::printf("LIBCXX-FUTURE-FAIL: promise value\n");
        return 1;
    }
    std::printf("LIBCXX-FUTURE-PASS\n");

    // std::atomic through the same header chain.
    std::atomic<int> finished{0};
    std::thread counter([&finished]() { finished.fetch_add(1); });
    counter.join();
    if (finished.load() != 1) {
        std::printf("LIBCXX-THREAD-FAIL: atomic\n");
        return 1;
    }

    // std::ofstream/std::ifstream on the guest VFS.
    const char* path = "/fstream-probe.txt";
    {
        std::ofstream file(path, std::ios::out | std::ios::trunc);
        if (!file.is_open()) {
            std::printf("LIBCXX-FSTREAM-FAIL: open for write\n");
            return 1;
        }
        file << "line-one\n" << 1234 << '\n' << std::hex << 255 << '\n';
        if (!file.good()) {
            std::printf("LIBCXX-FSTREAM-FAIL: write\n");
            return 1;
        }
    }
    {
        std::ifstream file(path);
        if (!file.is_open()) {
            std::printf("LIBCXX-FSTREAM-FAIL: open for read\n");
            return 1;
        }
        std::string first_line;
        int number = 0;
        std::getline(file, first_line);
        file >> number;
        if (first_line != "line-one" || number != 1234) {
            std::printf("LIBCXX-FSTREAM-FAIL: read back\n");
            return 1;
        }
    }
    std::remove(path);
    std::printf("LIBCXX-FSTREAM-PASS\n");

    std::printf("LIBCXX-THREAD-SUITE-PASS\n");
    return 0;
}
