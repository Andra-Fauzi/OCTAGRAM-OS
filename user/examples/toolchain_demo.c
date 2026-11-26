/*
 * Guest probe for the process/table/temporary-file layer of libc.
 *
 * A build driver shells out (`popen`), keeps symbol tables (`hsearch`), makes
 * scratch files with preserved suffixes (`mkostemp`/`mkstemps`/`mkostemps`),
 * and wants real randomness for temp names (`arc4random`). It also maps files
 * through the LFS name (`mmap64`). This probe exercises all of them.
 */

#include <dlfcn.h>
#include <errno.h>
#include <execinfo.h>
#include <fcntl.h>
#include <malloc.h>
#include <pthread.h>
#include <regex.h>
#include <semaphore.h>
#include <search.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/sysinfo.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include <unwind.h>

static int failures;

static void check(int condition, const char *what)
{
    if (!condition) {
        printf("toolchain: FAIL %s\n", what);
        failures++;
    }
}

/* ---- popen/pclose ------------------------------------------------------- */

static void test_popen_read(void)
{
    FILE *stream = popen("echo hello-from-popen", "r");
    check(stream != NULL, "popen read opened the pipeline");
    if (stream == NULL) {
        return;
    }
    char line[64];
    memset(line, 0, sizeof(line));
    check(fgets(line, sizeof(line), stream) != NULL, "popen read a line");
    check(strcmp(line, "hello-from-popen\n") == 0, "popen read the child output");
    check(pclose(stream) == 0, "pclose reported a successful reader");
}

static void test_popen_write(void)
{
    const char *path = "/tmp/popen-written.txt";
    (void)unlink(path);
    FILE *stream = popen("cat > /tmp/popen-written.txt", "w");
    check(stream != NULL, "popen write opened the pipeline");
    if (stream == NULL) {
        return;
    }
    check(fputs("written by the child\n", stream) >= 0, "popen write accepted data");
    const int status = pclose(stream);
    check(status == 0, "pclose reported a successful child");

    FILE *checker = fopen(path, "r");
    check(checker != NULL, "child created the output file");
    if (checker == NULL) {
        return;
    }
    char line[64];
    memset(line, 0, sizeof(line));
    check(fgets(line, sizeof(line), checker) != NULL, "read back child output");
    check(strcmp(line, "written by the child\n") == 0, "child output content");
    fclose(checker);
    unlink(path);

    check(popen("echo ignored", "x") == NULL, "popen rejects an invalid mode");
}

/* ---- hsearch ------------------------------------------------------------ */

static void test_hsearch(void)
{
    check(hcreate(8) != 0, "hcreate");
    ENTRY item;
    item.key = "alpha";
    item.data = (void *)1;
    check(hsearch(item, ENTER) != NULL, "hsearch ENTER inserts");
    item.data = (void *)2;
    ENTRY *stored = hsearch(item, FIND);
    check(stored != NULL && stored->data == (void *)1,
          "hsearch FIND returns the original stored entry");
    check(hsearch(item, ENTER) != NULL && hsearch(item, FIND)->data == (void *)1,
          "hsearch ENTER preserves data for an existing key");

    ENTRY missing;
    missing.key = "omega";
    missing.data = NULL;
    check(hsearch(missing, FIND) == NULL, "hsearch FIND misses an unknown key");

    /* Enough inserts to force at least one bucket growth. */
    static char names[64][16];
    for (int index = 0; index < 64; ++index) {
        snprintf(names[index], sizeof(names[index]), "key-%d", index);
        ENTRY entry;
        entry.key = names[index];
        entry.data = (void *)(long)(index + 1);
        check(hsearch(entry, ENTER) != NULL, "hsearch growth insert");
    }
    ENTRY late;
    late.key = names[63];
    late.data = NULL;
    check(hsearch(late, FIND) != NULL && hsearch(late, FIND)->data == (void *)64,
          "entry survives table growth");
    hdestroy();

    /* Reentrant form keeps its table in the caller's struct. */
    struct hsearch_data table;
    memset(&table, 0, sizeof(table));
    check(hcreate_r(4, &table) != 0, "hcreate_r");
    ENTRY entry;
    entry.key = "beta";
    entry.data = (void *)7;
    ENTRY *found = NULL;
    check(hsearch_r(entry, ENTER, &found, &table) != 0 && found != NULL,
          "hsearch_r ENTER");
    check(hsearch_r(entry, FIND, &found, &table) != 0 && found->data == (void *)7,
          "hsearch_r FIND");
    ENTRY unknown;
    unknown.key = "gamma";
    unknown.data = NULL;
    check(hsearch_r(unknown, FIND, &found, &table) == 0, "hsearch_r miss");
    hdestroy_r(&table);
}

/* ---- temporary files ---------------------------------------------------- */

static void test_temporary_files(void)
{
    char with_suffix[] = "/tmp/octagram-XXXXXX.o";
    const int descriptor = mkstemps(with_suffix, 2);
    check(descriptor >= 0, "mkstemps created a file");
    if (descriptor >= 0) {
        check(strcmp(with_suffix + strlen(with_suffix) - 2, ".o") == 0,
              "mkstemps preserved the suffix");
        check(write(descriptor, "obj", 3) == 3, "mkstemps file is writable");
        close(descriptor);
        unlink(with_suffix);
    }

    char cloexec[] = "/tmp/octagram-cloexec-XXXXXX";
    const int cloexec_descriptor = mkostemp(cloexec, O_CLOEXEC);
    check(cloexec_descriptor >= 0, "mkostemp created a file");
    if (cloexec_descriptor >= 0) {
        const int flags = fcntl(cloexec_descriptor, F_GETFD);
        check(flags >= 0, "mkostemp descriptor is queryable");
        close(cloexec_descriptor);
        unlink(cloexec);
    }

    char both[] = "/tmp/octagram-both-XXXXXX.txt";
    const int both_descriptor = mkostemps(both, 4, O_CLOEXEC);
    check(both_descriptor >= 0, "mkostemps created a file");
    if (both_descriptor >= 0) {
        check(strcmp(both + strlen(both) - 4, ".txt") == 0,
              "mkostemps preserved the suffix");
        close(both_descriptor);
        unlink(both);
    }

    check(mkstemps(with_suffix, -1) == -1, "mkstemps rejects a negative suffix");
}

/* ---- arc4random --------------------------------------------------------- */

static void test_arc4random(void)
{
    unsigned char first[32];
    unsigned char second[32];
    memset(first, 0, sizeof(first));
    memset(second, 0, sizeof(second));
    arc4random_buf(first, sizeof(first));
    arc4random_buf(second, sizeof(second));
    check(memcmp(first, second, sizeof(first)) != 0,
          "two buffers differ");

    int distinct = 0;
    for (int index = 0; index < 32; ++index) {
        if (first[index] != 0) {
            distinct = 1;
        }
    }
    check(distinct != 0, "buffer is not all zeroes");

    check(arc4random_uniform(1) == 0, "uniform(1) is zero");
    int out_of_range = 0;
    for (int index = 0; index < 256; ++index) {
        const uint32_t value = arc4random_uniform(7);
        if (value >= 7) {
            out_of_range = 1;
        }
    }
    check(out_of_range == 0, "uniform bound respected");
}

/* ---- mmap64 ------------------------------------------------------------- */

static void test_mmap64(void)
{
    const size_t length = 4096;
    unsigned char *mapping =
        mmap64(NULL, length, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS,
               -1, 0);
    check(mapping != MAP_FAILED, "mmap64 mapped an anonymous page");
    if (mapping == MAP_FAILED) {
        return;
    }
    mapping[0] = 0x5a;
    mapping[length - 1] = 0x2b;
    check(mapping[0] == 0x5a && mapping[length - 1] == 0x2b,
          "mmap64 mapping is readable and writable");
    check(munmap(mapping, length) == 0, "munmap released the mapping");
}

/* ---- regex -------------------------------------------------------------- */

static void check_regex(const char *pattern, int flags, const char *text,
                        long expected_start, long expected_end, const char *what)
{
    regex_t expression;
    check(regcomp(&expression, pattern, flags) == 0, what);
    regmatch_t matches[4];
    const int status = regexec(&expression, text, 4, matches, 0);
    if (expected_start < 0) {
        check(status == REG_NOMATCH, what);
    } else {
        check(status == 0 && matches[0].rm_so == expected_start &&
                  matches[0].rm_eo == expected_end,
              what);
    }
    regfree(&expression);
}

static void test_regex(void)
{
    check_regex("abc", REG_EXTENDED, "xabcx", 1, 4, "ERE literal");
    check_regex("a+b", REG_EXTENDED, "aaab", 0, 4, "ERE plus");
    check_regex(".*", REG_EXTENDED, "abc", 0, 3, "ERE dot star");
    check_regex("a|ab", REG_EXTENDED, "ab", 0, 2, "ERE leftmost-longest");
    check_regex("^foo$", REG_EXTENDED, "foo", 0, 3, "ERE anchors");
    check_regex("[[:digit:]]+", REG_EXTENDED, "ab123cd", 2, 5, "ERE class");
    check_regex("[^0-9]+", REG_EXTENDED, "12ab34", 2, 4, "ERE negated class");
    check_regex("a{2,3}", REG_EXTENDED, "aaaa", 0, 3, "ERE interval");
    check_regex("hello", REG_ICASE, "HeLLo", 0, 5, "REG_ICASE");
    check_regex("[abc]", REG_EXTENDED, "xyz", -1, 0,
                "ERE bracket class does not match unrelated text");
    check_regex("\\(ab\\)*c", 0, "ababc", 0, 5, "BRE group star");
    check_regex("\\(a\\)\\1", 0, "xaay", 1, 3, "BRE back reference");
    check_regex("a\\{2\\}", 0, "xaay", 1, 3, "BRE interval");

    regex_t expression;
    check(regcomp(&expression, "(a)(b)?", REG_EXTENDED) == 0, "group compile");
    regmatch_t matches[3];
    check(regexec(&expression, "a", 3, matches, 0) == 0, "optional group exec");
    check(matches[1].rm_so == 0 && matches[1].rm_eo == 1, "group 1 span");
    check(matches[2].rm_so == -1 && matches[2].rm_eo == -1,
          "unmatched group reports -1");
    check(expression.re_nsub == 2, "re_nsub counts the groups");
    regfree(&expression);

    char message[64];
    check(regerror(REG_BADPAT, NULL, message, sizeof(message)) > 0,
          "regerror returns a length");
    check(strstr(message, "regular expression") != NULL, "regerror text");

    check(regcomp(&expression, "(a*)*b", REG_EXTENDED) == 0, "empty-body loop");
    check(regexec(&expression, "aaab", 1, matches, 0) == 0 &&
              matches[0].rm_eo == 4,
          "empty-body loop terminates");
    regfree(&expression);

    check(regcomp(&expression, "a[", REG_EXTENDED) == REG_EBRACK,
          "regcomp reports REG_EBRACK");
    check(regcomp(&expression, "(ab", REG_EXTENDED) == REG_EPAREN,
          "regcomp reports REG_EPAREN");
    check(regcomp(&expression, "a{3,1}", REG_EXTENDED) == REG_BADBR,
          "regcomp reports REG_BADBR");
    check(regcomp(&expression, "a\\", REG_EXTENDED) == REG_EESCAPE,
          "regcomp reports REG_EESCAPE");
}

/* ---- semaphores --------------------------------------------------------- */

static sem_t worker_semaphore;
static int worker_result;

static void *semaphore_worker(void *argument)
{
    (void)argument;
    worker_result = 1;
    sem_post(&worker_semaphore);
    return NULL;
}

static void test_semaphores(void)
{
    sem_t semaphore;
    check(sem_init(&semaphore, 0, 1) == 0, "sem_init");

    int value = -1;
    check(sem_getvalue(&semaphore, &value) == 0 && value == 1,
          "sem_getvalue reports the initial token");
    check(sem_wait(&semaphore) == 0, "sem_wait takes the token");
    check(sem_trywait(&semaphore) == -1 && errno == EAGAIN,
          "sem_trywait reports an empty semaphore");

    struct timespec deadline;
    check(clock_gettime(CLOCK_REALTIME, &deadline) == 0, "clock_gettime");
    deadline.tv_sec -= 1;
    check(sem_timedwait(&semaphore, &deadline) == -1 && errno == ETIMEDOUT,
          "sem_timedwait times out on an empty semaphore");

    check(sem_post(&semaphore) == 0, "sem_post releases a token");
    check(sem_timedwait(&semaphore, &deadline) == 0,
          "sem_timedwait takes an available token");
    check(sem_destroy(&semaphore) == 0, "sem_destroy");

    /* A waiter really sleeps until another thread posts. */
    check(sem_init(&worker_semaphore, 0, 0) == 0, "sem_init for the worker");
    worker_result = 0;
    pthread_t thread;
    check(pthread_create(&thread, NULL, semaphore_worker, NULL) == 0,
          "pthread_create for the semaphore worker");
    check(sem_wait(&worker_semaphore) == 0, "sem_wait blocks until the post");
    check(worker_result == 1, "the worker ran before the token arrived");
    pthread_join(thread, NULL);
    sem_destroy(&worker_semaphore);

    /* Named semaphores are process-local. */
    sem_t *named = sem_open("/octagram-probe", O_CREAT, 0600, 2);
    check(named != SEM_FAILED, "sem_open creates a named semaphore");
    value = -1;
    check(sem_getvalue(named, &value) == 0 && value == 2,
          "named semaphore carries the initial value");
    check(sem_open("/octagram-probe", O_CREAT | O_EXCL, 0600, 1) == SEM_FAILED &&
              errno == EEXIST,
          "O_EXCL rejects an existing name");
    check(sem_close(named) == 0, "sem_close");
    check(sem_unlink("/octagram-probe") == 0, "sem_unlink");
    check(sem_open("/octagram-probe", 0) == SEM_FAILED && errno == ENOENT,
          "opening without O_CREAT fails after unlink");
}

/* ---- allocator helpers -------------------------------------------------- */

static void test_allocator_helpers(void)
{
    const int page = getpagesize();
    check(page >= 4096, "getpagesize reports a plausible page size");

    void *block = valloc(64);
    check(block != NULL && ((uintptr_t)block % (uintptr_t)page) == 0,
          "valloc aligns to the page size");
    free(block);

    block = pvalloc(1);
    check(block != NULL && ((uintptr_t)block % (uintptr_t)page) == 0,
          "pvalloc aligns to the page size");
    free(block);

    block = memalign(64, 100);
    check(block != NULL && ((uintptr_t)block % 64u) == 0,
          "memalign honours the requested alignment");
    cfree(block);

    check(memalign(3, 8) == NULL && errno == EINVAL,
          "memalign rejects a non power of two");
    check(mallopt(M_TRIM_THRESHOLD, 1024) == 0,
          "mallopt reports that tuning is unsupported");
    check(malloc_trim(0) == 0, "malloc_trim reports nothing released");
}

/* ---- time zone ---------------------------------------------------------- */

static void test_timezone(void)
{
    setenv("TZ", "WIB-7", 1);
    tzset();
    check(strcmp(tzname[0], "WIB") == 0, "tzset parsed the standard name");
    check(timezone == -7 * 3600, "timezone is seconds west of UTC");
    check(daylight == 0, "no daylight rule for a fixed zone");

    const time_t instant = 1700000000; /* 2023-11-14T22:13:20Z */
    struct tm local;
    check(localtime_r(&instant, &local) != NULL, "localtime_r for UTC+7");
    check(local.tm_hour == 5 && local.tm_min == 13 && local.tm_mday == 15,
          "UTC+7 shifts the wall clock past midnight");
    check(local.tm_gmtoff == 7 * 3600, "tm_gmtoff carries the offset");
    check(local.tm_zone != NULL && strcmp(local.tm_zone, "WIB") == 0,
          "tm_zone carries the name");

    char rendered[40];
    rendered[0] = '\0';
    strftime(rendered, sizeof(rendered), "%Y-%m-%d %H:%M %z %Z", &local);
    check(strcmp(rendered, "2023-11-15 05:13 +0700 WIB") == 0,
          "strftime renders %z and %Z from the zone");

    struct tm round_trip = local;
    check(mktime(&round_trip) == instant, "mktime inverts localtime for a fixed zone");

    /* A DST zone with POSIX rules: US Eastern. */
    setenv("TZ", "EST5EDT,M3.2.0,M11.1.0", 1);
    tzset();
    check(strcmp(tzname[0], "EST") == 0 && strcmp(tzname[1], "EDT") == 0,
          "standard and daylight names");
    check(timezone == 5 * 3600 && daylight == 1, "EST offset and daylight flag");

    const time_t summer = 1719835200; /* 2024-07-01T12:00:00Z */
    check(localtime_r(&summer, &local) != NULL, "localtime_r in summer");
    check(local.tm_hour == 8 && local.tm_isdst == 1 &&
              local.tm_gmtoff == -4 * 3600,
          "summer uses EDT (UTC-4)");
    struct tm summer_round_trip = local;
    check(mktime(&summer_round_trip) == summer, "mktime round trip with DST");

    const time_t winter = 1705320000; /* 2024-01-15T12:00:00Z */
    check(localtime_r(&winter, &local) != NULL, "localtime_r in winter");
    check(local.tm_hour == 7 && local.tm_isdst == 0 &&
              local.tm_gmtoff == -5 * 3600,
          "winter uses EST (UTC-5)");
    struct tm winter_round_trip = local;
    check(mktime(&winter_round_trip) == winter, "mktime round trip in winter");

    /* A zone file path is not a POSIX TZ string: fall back to UTC. */
    setenv("TZ", "/usr/share/zoneinfo/Asia/Jakarta", 1);
    tzset();
    check(timezone == 0 && strcmp(tzname[0], "UTC") == 0,
          "unparsable TZ falls back to UTC");
    check(localtime_r(&instant, &local) != NULL && local.tm_hour == 22,
          "fallback keeps UTC wall clock");
    unsetenv("TZ");
    tzset();
}

/* ---- strptime ----------------------------------------------------------- */

static void test_strptime(void)
{
    struct tm value;
    memset(&value, 0, sizeof(value));
    const char *rest = strptime("2026-11-25 13:45:07 tail", "%Y-%m-%d %H:%M:%S",
                                &value);
    check(rest != NULL, "strptime parsed a timestamp");
    if (rest != NULL) {
        check(strcmp(rest, " tail") == 0, "strptime stopped after the timestamp");
        check(value.tm_year == 126, "strptime stored the year offset");
        check(value.tm_mon == 10, "strptime stored the month");
        check(value.tm_mday == 25, "strptime stored the day");
        check(value.tm_hour == 13 && value.tm_min == 45 && value.tm_sec == 7,
              "strptime stored the time of day");
    }

    memset(&value, 0, sizeof(value));
    rest = strptime("Wed, 25 Nov 2026 23:30:00 GMT", "%a, %d %b %Y %H:%M:%S %Z",
                    &value);
    check(rest != NULL && *rest == '\0', "strptime parsed an HTTP date");
    if (rest != NULL) {
        check(value.tm_mon == 10 && value.tm_mday == 25 && value.tm_hour == 23,
              "HTTP date fields");
        check(value.tm_wday == 3, "strptime stored the weekday name");
    }

    memset(&value, 0, sizeof(value));
    rest = strptime("11:05 pm", "%I:%M %p", &value);
    check(rest != NULL && value.tm_hour == 23 && value.tm_min == 5,
          "12-hour clock with pm");

    memset(&value, 0, sizeof(value));
    rest = strptime("12:30 am", "%I:%M %p", &value);
    check(rest != NULL && value.tm_hour == 0, "12-hour clock midnight wrap");

    memset(&value, 0, sizeof(value));
    check(strptime("not-a-date", "%Y-%m-%d", &value) == NULL,
          "strptime rejects mismatched input");
    check(strptime("2026-11-25", "%Y-%m-%d %H:%M", &value) == NULL,
          "strptime rejects truncated input");

    /* Round trip through strftime. */
    char rendered[32];
    memset(&value, 0, sizeof(value));
    value.tm_year = 126;
    value.tm_mon = 10;
    value.tm_mday = 25;
    const size_t written = strftime(rendered, sizeof(rendered), "%Y-%m-%d", &value);
    check(written == 10, "strftime rendered ten characters");
    struct tm reparsed;
    memset(&reparsed, 0, sizeof(reparsed));
    check(strptime(rendered, "%Y-%m-%d", &reparsed) != NULL,
          "strptime read the rendered date back");
    check(reparsed.tm_year == value.tm_year && reparsed.tm_mon == value.tm_mon &&
              reparsed.tm_mday == value.tm_mday,
          "round trip preserved the date");
}

/* ---- stack unwinding ---------------------------------------------------- */

struct unwind_probe {
    int frames;
    uintptr_t first_ip;
};

static _Unwind_Reason_Code unwind_probe_callback(struct _Unwind_Context *context,
                                                  void *argument)
{
    struct unwind_probe *state = argument;
    uintptr_t ip = _Unwind_GetIP(context);
    int before = -1;
    if (ip == 0) {
        return _URC_END_OF_STACK;
    }
    if (_Unwind_GetIPInfo(context, &before) != ip || before != 0) {
        return _URC_FATAL_PHASE1_ERROR;
    }
    if (_Unwind_GetCFA(context) == 0) {
        return _URC_FATAL_PHASE1_ERROR;
    }
    if (state->frames == 0) {
        state->first_ip = ip;
    }
    state->frames++;
    return _URC_NO_REASON;
}

__attribute__((noinline)) static int unwind_inner(void *frames, int size)
{
    return backtrace(frames, size);
}

__attribute__((noinline)) static int unwind_middle(void *frames, int size)
{
    return unwind_inner(frames, size);
}

static void test_backtrace(void)
{
    struct unwind_probe probe;
    probe.frames = 0;
    probe.first_ip = 0;
    _Unwind_Reason_Code code = _Unwind_Backtrace(unwind_probe_callback, &probe);
    check(code == _URC_END_OF_STACK, "_Unwind_Backtrace ends at the stack base");
    check(probe.frames >= 2, "_Unwind_Backtrace visited the caller frames");
    check(probe.first_ip != 0, "_Unwind_Backtrace reported an instruction pointer");

    void *frames[32];
    int count = unwind_middle(frames, 32);
    check(count >= 3, "backtrace returned the call chain");
    check(count <= 32, "backtrace respects the buffer size");
    int complete = 1;
    for (int index = 0; index < count; ++index) {
        if (frames[index] == NULL) {
            complete = 0;
        }
    }
    check(complete, "every recorded frame carries an address");

    char **symbols = backtrace_symbols(frames, count);
    check(symbols != NULL, "backtrace_symbols allocates its table");
    if (symbols != NULL) {
        check(symbols[0] != NULL && strncmp(symbols[0], "#0 0x", 5) == 0,
              "backtrace_symbols formats the first frame");
        check(symbols[count - 1] != NULL, "backtrace_symbols fills the last row");
        free(symbols);
    }

    const char *path = "/tmp/backtrace-probe.txt";
    int descriptor = open(path, O_CREAT | O_TRUNC | O_WRONLY, 0600);
    check(descriptor >= 0, "backtrace_symbols_fd target opened");
    if (descriptor >= 0) {
        backtrace_symbols_fd(frames, count, descriptor);
        close(descriptor);
        struct stat information;
        check(stat(path, &information) == 0 && information.st_size > 0,
              "backtrace_symbols_fd wrote one line per frame");
        unlink(path);
    }

    /* A buffer that cannot hold the whole chain still returns a prefix. */
    void *two[2];
    int truncated = unwind_middle(two, 2);
    check(truncated == 2, "backtrace stops at the buffer size");
}

/* ---- dladdr ------------------------------------------------------------- */

static void test_dladdr(void)
{
    Dl_info information;
    memset(&information, 0x5a, sizeof(information));
    int found = dladdr((const void *)(uintptr_t)&test_dladdr, &information);
    if (found) {
        check(information.dli_fname != NULL, "dladdr names the owning object");
        check(information.dli_fbase != NULL, "dladdr reports the load base");
    } else {
        check(information.dli_fname == NULL && information.dli_fbase == NULL
                  && information.dli_sname == NULL
                  && information.dli_saddr == NULL,
              "dladdr clears the result when it cannot attribute the address");
    }
    check(dladdr(NULL, &information) == 0, "dladdr rejects a NULL address");
}

/* ---- processor count ---------------------------------------------------- */

static void test_processor_count(void)
{
    int online = get_nprocs();
    int configured = get_nprocs_conf();
    check(online >= 1, "get_nprocs reports at least one processor");
    check(configured >= online,
          "get_nprocs_conf never reports fewer than get_nprocs");
}

int main(void)
{
    test_popen_read();
    test_popen_write();
    test_hsearch();
    test_temporary_files();
    test_arc4random();
    test_mmap64();
    test_regex();
    test_timezone();
    test_semaphores();
    test_allocator_helpers();
    test_strptime();
    test_backtrace();
    test_dladdr();
    test_processor_count();

    if (failures != 0) {
        printf("TOOLCHAIN-FAIL: %d check(s) failed\n", failures);
        return 1;
    }
    printf("TOOLCHAIN-PASS\n");
    return 0;
}
