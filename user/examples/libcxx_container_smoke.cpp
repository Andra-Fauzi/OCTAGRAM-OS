// Guest probe for the container/utility half of the OCTAGRAM libc++ build.
//
// Covers what a compiler front end actually leans on: ordered and unordered
// associative containers with std::hash<std::string>, node containers,
// std::valarray, std::bitset, the C++17 vocabulary types, std::function and
// std::bind, std::atomic, and std::shared_mutex/shared_lock on top of the new
// pthread_rwlock implementation.
//
// Prints LIBCXX-CONTAINER-PASS only when every check succeeded.

#include <atomic>
#include <bitset>
#include <deque>
#include <forward_list>
#include <functional>
#include <iostream>
#include <list>
#include <map>
#include <memory>
#include <optional>
#include <set>
#include <shared_mutex>
#include <span>
#include <sstream>
#include <string>
#include <string_view>
#include <thread>
#include <tuple>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <valarray>
#include <variant>
#include <vector>

namespace {

int failures = 0;

void check(bool condition, const char* what)
{
    if (!condition) {
        std::cout << "FAIL " << what << "\n";
        ++failures;
    }
}

void check_ordered_containers()
{
    std::map<std::string, int> counts;
    for (const char* word : {"alpha", "beta", "alpha", "gamma", "beta", "alpha"}) {
        ++counts[word];
    }
    check(counts.size() == 3, "map size");
    check(counts["alpha"] == 3, "map operator[]");
    check(counts.begin()->first == "alpha", "map ordering");
    counts.erase("gamma");
    check(counts.count("gamma") == 0, "map erase");

    std::set<int> unique{5, 3, 9, 3, 1};
    check(unique.size() == 4, "set dedupe");
    check(*unique.begin() == 1 && *unique.rbegin() == 9, "set iteration order");
    check(unique.lower_bound(4) != unique.end() && *unique.lower_bound(4) == 5,
          "set lower_bound");

    std::multimap<int, std::string> buckets;
    buckets.emplace(1, "one");
    buckets.emplace(1, "uno");
    buckets.emplace(2, "two");
    check(buckets.count(1) == 2, "multimap duplicate keys");

    // std::hash<std::string> lives in hash.cpp; without it these containers
    // silently degrade to a single bucket (or fail to link at all).
    std::unordered_map<std::string, int> hashed;
    for (int i = 0; i < 64; ++i) {
        hashed.emplace("key-" + std::to_string(i), i);
    }
    check(hashed.size() == 64, "unordered_map size");
    check(hashed.at("key-42") == 42, "unordered_map lookup");
    check(hashed.bucket_count() > 1, "unordered_map uses more than one bucket");
    hashed.erase("key-42");
    check(hashed.find("key-42") == hashed.end(), "unordered_map erase");

    std::unordered_set<std::string> seen{"a", "b", "a"};
    check(seen.size() == 2, "unordered_set dedupe");
}

void check_sequence_containers()
{
    std::deque<int> queue{2, 3};
    queue.push_front(1);
    queue.push_back(4);
    check(queue.front() == 1 && queue.back() == 4, "deque ends");
    check(queue.size() == 4, "deque size");

    std::list<int> list{3, 1, 2};
    list.sort();
    list.reverse();
    check(list.front() == 3 && list.back() == 1, "list sort/reverse");

    std::forward_list<int> forward{1, 2, 3};
    forward.push_front(0);
    int sum = 0;
    for (int value : forward) {
        sum += value;
    }
    check(sum == 6, "forward_list sum");

    std::vector<std::string> words{"delta", "alpha", "charlie", "bravo"};
    std::sort(words.begin(), words.end());
    check(words.front() == "alpha" && words.back() == "delta", "vector sort");
    words.insert(words.begin() + 1, "aardvark");
    check(words[1] == "aardvark" && words.size() == 5, "vector insert");
}

void check_valarray_and_bitset()
{
    std::valarray<double> values{1.0, 2.0, 3.0, 4.0};
    std::valarray<double> doubled = values * 2.0;
    check(doubled.sum() == 20.0, "valarray scale+sum");
    check(values.max() == 4.0 && values.min() == 1.0, "valarray min/max");
    std::valarray<double> shifted = values.shift(1);
    // A positive valarray shift moves elements toward index zero, then
    // zero-fills the vacated tail: {2, 3, 4, 0}.
    check(shifted[0] == 2.0 && shifted[1] == 3.0 &&
              shifted[2] == 4.0 && shifted[3] == 0.0,
          "valarray shift");
    double dot = (values * values).sum();
    check(dot == 30.0, "valarray dot");

    std::bitset<13> bits;
    bits.set(0);
    bits.set(3);
    bits.set(12);
    check(bits.count() == 3, "bitset count");
    check(bits.test(12) && !bits.test(1), "bitset test");
    bits.flip();
    check(bits.count() == 10, "bitset flip");
    check(bits.to_string().size() == 13, "bitset to_string");
    check((std::bitset<8>(0b1010'1010) & std::bitset<8>(0b1111'0000)).to_ulong() ==
              0b1010'0000ul,
          "bitset bitwise and");
}

struct Point {
    int x;
    int y;
    bool operator==(const Point& other) const { return x == other.x && y == other.y; }
};

void check_vocabulary_types()
{
    std::optional<std::string> maybe;
    check(!maybe.has_value(), "optional empty");
    maybe = "octagram";
    check(maybe.has_value() && *maybe == "octagram", "optional value");
    check(maybe.value_or("fallback") == "octagram", "optional value_or");

    std::variant<int, std::string, Point> variant = 17;
    check(std::holds_alternative<int>(variant), "variant holds int");
    check(std::get<int>(variant) == 17, "variant get");
    variant = std::string("native");
    check(std::holds_alternative<std::string>(variant), "variant reassigned");
    check(std::get<std::string>(variant) == "native", "variant string get");
    variant = Point{3, 4};
    check(std::get<Point>(variant).y == 4, "variant struct");
    check(std::visit([](const auto& value) { return sizeof(value) > 0; }, variant),
          "variant visit");

    auto tuple = std::make_tuple(1, std::string("two"), 3.0);
    check(std::get<2>(tuple) == 3.0, "tuple get");

    std::string_view view("self-hosting");
    check(view.substr(0, 4) == "self", "string_view substr");
    check(view.find('-') == 4, "string_view find");

    int raw[] = {1, 2, 3, 4};
    std::span<int> span(raw);
    check(span.size() == 4 && span.back() == 4, "span");
    check(span.first(2).size() == 2, "span first");
}

int add(int left, int right) { return left + right; }

void check_function_objects()
{
    std::function<int(int, int)> sum = add;
    check(sum(2, 3) == 5, "function from free function");

    std::function<int(int)> increment = [](int value) { return value + 1; };
    check(increment(41) == 42, "function from lambda");

    auto bound = std::bind(add, std::placeholders::_1, 10);
    check(bound(5) == 15, "bind placeholder");

    auto shared = std::make_shared<std::string>("shared");
    std::weak_ptr<std::string> observer = shared;
    check(!observer.expired() && observer.lock()->size() == 6, "weak_ptr lock");
    shared.reset();
    check(observer.expired(), "weak_ptr expires");

    std::unique_ptr<std::vector<int>> owned = std::make_unique<std::vector<int>>(3, 7);
    check(owned->size() == 3 && (*owned)[0] == 7, "unique_ptr");

    std::atomic<int> counter{0};
    counter.fetch_add(3);
    counter += 4;
    check(counter.load() == 7, "atomic add");
    check(counter.exchange(1) == 7, "atomic exchange");
}

void check_shared_mutex()
{
    std::shared_mutex mutex;
    std::vector<int> observed;
    int published = 0;

    std::thread reader([&] {
        std::shared_lock<std::shared_mutex> guard(mutex);
        observed.push_back(published);
    });
    reader.join();
    check(observed.size() == 1 && observed[0] == 0, "shared_lock read");

    {
        std::unique_lock<std::shared_mutex> guard(mutex);
        published = 9;
    }
    std::thread reader2([&] {
        std::shared_lock<std::shared_mutex> guard(mutex);
        observed.push_back(published);
    });
    reader2.join();
    check(observed.back() == 9, "shared_lock sees writer");

    // Two concurrent readers must both be admitted; the writer must wait.
    std::atomic<int> readers_inside{0};
    std::atomic<bool> release{false};
    auto reader_body = [&] {
        std::shared_lock<std::shared_mutex> guard(mutex);
        readers_inside.fetch_add(1);
        while (!release.load()) {
            std::this_thread::yield();
        }
        readers_inside.fetch_sub(1);
    };
    std::thread first(reader_body);
    std::thread second(reader_body);
    while (readers_inside.load() < 2) {
        std::this_thread::yield();
    }
    check(mutex.try_lock() == false, "writer blocked while readers hold");
    release.store(true);
    first.join();
    second.join();
    check(readers_inside.load() == 0, "readers released");
    check(mutex.try_lock() == true, "writer acquired after drain");
    mutex.unlock();
}

} // namespace

int main()
{
    std::cout << "libc++ container/utility probe on OCTAGRAM\n";
    check_ordered_containers();
    check_sequence_containers();
    check_valarray_and_bitset();
    check_vocabulary_types();
    check_function_objects();
    check_shared_mutex();

    if (failures != 0) {
        std::cout << "LIBCXX-CONTAINER-FAIL " << failures << "\n";
        return 1;
    }
    std::cout << "LIBCXX-CONTAINER-PASS\n";
    return 0;
}
