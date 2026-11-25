/* Exercises the compatibility syscalls added for Python/Java runtimes.
 * Run as `/bin/syscall-compat-demo` from the OCTAGRAM shell. */
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <octagram.h>
#include <poll.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/select.h>
#include <sys/epoll.h>
#include <sys/eventfd.h>
#include <signal.h>
#include <sys/random.h>
#include <locale.h>
#include <termios.h>
#include <uchar.h>
#include <wchar.h>
#include <sys/ioctl.h>
#include <sys/semaphore.h>
#include <sys/socket.h>
#include <sys/timerfd.h>
#include <sys/time.h>
#include <sys/utsname.h>
#include <time.h>
#include <unistd.h>

static int failures;

static void check(int condition, const char *what)
{
    if (condition) {
        printf("ok   %s\n", what);
    } else {
        printf("FAIL %s\n", what);
        ++failures;
    }
}

#define FUTEX_WAIT 0
#define FUTEX_WAKE 1
#define FUTEX_REQUEUE 3
#define FUTEX_WAIT_BITSET 9
#define FUTEX_WAKE_BITSET 10
#define FUTEX_CLOCK_REALTIME 256

static uint32_t futex_word;
static volatile int futex_result;
static volatile int futex_timeout_started;
static uint32_t futex_bitset_word;
static volatile int futex_bitset_result;
static uint32_t futex_requeue_source;
static uint32_t futex_requeue_target;
static volatile int futex_requeue_result;

static void *futex_waiter(void *argument)
{
    (void)argument;
    futex_result = (int)oct_futex(&futex_word, FUTEX_WAIT, 0);
    return NULL;
}

static void *futex_timeout_waiter(void *argument)
{
    (void)argument;
    struct timespec timeout;
    timeout.tv_sec = 0;
    timeout.tv_nsec = 50 * 1000 * 1000;
    __sync_lock_test_and_set(&futex_timeout_started, 1);
    futex_result = (int)oct_futex_wait_timeout(
        &futex_word, 0, (const struct oct_timespec *)&timeout);
    return NULL;
}

static void *futex_bitset_waiter(void *argument)
{
    (void)argument;
    futex_bitset_result = (int)oct_syscall6(
        94, (uint64_t)(uintptr_t)&futex_bitset_word,
        FUTEX_WAIT_BITSET, 0, 0, 0, 0x2);
    return NULL;
}

static void *futex_requeue_waiter(void *argument)
{
    (void)argument;
    futex_requeue_result = (int)oct_futex(
        &futex_requeue_source, FUTEX_WAIT, 0);
    return NULL;
}

static volatile int usr1_count;
static volatile int usr2_count;

static void user_signal_handler(int number)
{
    if (number == SIGUSR1) {
        usr1_count += 1;
    } else if (number == SIGUSR2) {
        usr2_count += 1;
    }
}

static int interrupt_pipe[2];
static volatile int interrupt_result;
static volatile int interrupt_errno;
static uint32_t interrupt_futex_word;

static void *blocked_pipe_reader(void *argument)
{
    (void)argument;
    char byte = 0;
    int result = (int)read(interrupt_pipe[0], &byte, 1);
    interrupt_errno = errno;
    interrupt_result = result;
    return NULL;
}

static void *blocked_pipe_poller(void *argument)
{
    (void)argument;
    struct pollfd descriptor;
    descriptor.fd = interrupt_pipe[0];
    descriptor.events = POLLIN;
    descriptor.revents = 0;
    int result = poll(&descriptor, 1, -1);
    interrupt_errno = errno;
    interrupt_result = result;
    return NULL;
}

static void *blocked_pipe_selector(void *argument)
{
    (void)argument;
    fd_set read_set;
    struct timeval timeout;
    FD_ZERO(&read_set);
    FD_SET(interrupt_pipe[0], &read_set);
    timeout.tv_sec = 5;
    timeout.tv_usec = 0;
    int result = select(interrupt_pipe[0] + 1, &read_set, NULL, NULL, &timeout);
    interrupt_errno = errno;
    interrupt_result = result;
    return NULL;
}

static void *blocked_futex_waiter(void *argument)
{
    (void)argument;
    int result = (int)oct_futex(&interrupt_futex_word, FUTEX_WAIT, 0);
    interrupt_errno = errno;
    interrupt_result = result;
    return NULL;
}


static uint64_t demo_monotonic_ms(void)
{
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    return (uint64_t)now.tv_sec * 1000u + (uint64_t)now.tv_nsec / 1000000u;
}

static pthread_mutex_t counter_mutex = PTHREAD_MUTEX_INITIALIZER;
static volatile int counter_value;

static void *counter_worker(void *argument)
{
    (void)argument;
    for (int i = 0; i < 500; ++i) {
        pthread_mutex_lock(&counter_mutex);
        int value = counter_value;
        counter_value = value + 1;
        pthread_mutex_unlock(&counter_mutex);
    }
    return NULL;
}

static pthread_mutex_t cond_mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cond_variable = PTHREAD_COND_INITIALIZER;
static volatile int cond_flag;
static volatile int cond_woken;

static void *cond_worker(void *argument)
{
    (void)argument;
    pthread_mutex_lock(&cond_mutex);
    while (cond_flag == 0) {
        pthread_cond_wait(&cond_variable, &cond_mutex);
    }
    ++cond_woken;
    pthread_mutex_unlock(&cond_mutex);
    return NULL;
}

static volatile unsigned long background_counter;
static volatile int background_stop;

static void *background_worker(void *argument)
{
    (void)argument;
    while (background_stop == 0) {
        __sync_fetch_and_add(&background_counter, 1);
    }
    return NULL;
}

static sem_t demo_semaphore;
static volatile int semaphore_result;

static void *semaphore_worker(void *argument)
{
    (void)argument;
    if (sem_wait(&demo_semaphore) == 0) {
        semaphore_result = 1;
    }
    return NULL;
}

int main(void)
{
    struct utsname name;
    memset(&name, 0, sizeof(name));
    check(uname(&name) == 0, "uname() == 0");
    printf("     sysname=%s nodename=%s release=%s machine=%s\n",
           name.sysname, name.nodename, name.release, name.machine);
    check(strcmp(name.sysname, "OCTAGRAM-OS") == 0, "uname sysname");
    check(strcmp(name.machine, "x86_64") == 0, "uname machine");

    struct timeval now;
    check(gettimeofday(&now, 0) == 0, "gettimeofday() == 0");
    check(now.tv_sec > 1600000000, "gettimeofday has Unix seconds");
    printf("     time=%lld\n", (long long)now.tv_sec);

    check(sysconf(_SC_PAGESIZE) == 4096, "sysconf(_SC_PAGESIZE)");
    check(sysconf(_SC_OPEN_MAX) == OPEN_MAX, "sysconf(_SC_OPEN_MAX)");
    check(sysconf(9999) == -1, "sysconf(unknown) == -1");

    struct timespec mono1;
    struct timespec mono2;
    struct timespec resolution;
    check(clock_gettime(CLOCK_MONOTONIC, &mono1) == 0,
          "clock_gettime(CLOCK_MONOTONIC)");
    check(mono1.tv_nsec >= 0 && mono1.tv_nsec < 1000000000,
          "monotonic nanoseconds in range");
    check(clock_getres(CLOCK_MONOTONIC, &resolution) == 0 &&
              resolution.tv_nsec > 0,
          "clock_getres(CLOCK_MONOTONIC)");
    check(clock_gettime(CLOCK_MONOTONIC, &mono2) == 0 &&
              (mono2.tv_sec > mono1.tv_sec ||
               (mono2.tv_sec == mono1.tv_sec && mono2.tv_nsec >= mono1.tv_nsec)),
          "CLOCK_MONOTONIC never goes backwards");

    check(getppid() == 1, "getppid() == 1");

    unsigned char random_a[16];
    unsigned char random_b[16];
    check(getrandom(random_a, sizeof(random_a), 0) == (ssize_t)sizeof(random_a),
          "getrandom() fills the buffer");
    check(getrandom(random_b, sizeof(random_b), 0) == (ssize_t)sizeof(random_b),
          "getrandom() works a second time");
    check(memcmp(random_a, random_b, sizeof(random_a)) != 0,
          "getrandom returns fresh bytes");

    int sync_file = open("/tmp/fsync-test", O_CREAT | O_WRONLY | O_TRUNC, 0644);
    check(setlocale(LC_ALL, NULL) != NULL, "setlocale(LC_ALL, NULL)");
    char *wide_locale = setlocale(LC_ALL, "C.UTF-8");
    check(wide_locale != NULL && strcmp(wide_locale, "C.UTF-8") == 0,
          "setlocale(C.UTF-8)");
    check(setlocale(LC_ALL, "xx_YY.UTF-8") == NULL,
          "setlocale(locale tak dikenal) == NULL");
    check(setlocale(LC_ALL, "C") != NULL, "setlocale(C)");
    struct lconv *conventions = localeconv();
    check(conventions != NULL &&
          strcmp(conventions->decimal_point, ".") == 0,
          "localeconv decimal_point == \".\"");
    check(conventions != NULL && strcmp(conventions->thousands_sep, "") == 0,
          "localeconv thousands_sep kosong");

    mbstate_t wide_state;
    wchar_t wide_character = 0;
    memset(&wide_state, 0, sizeof(wide_state));
    check(mbrtowc(&wide_character, "A", 1, &wide_state) == 1 &&
          wide_character == L'A', "mbrtowc ASCII");
    check(mbrtowc(&wide_character, "\xC3\xA9", 2, &wide_state) == 2 &&
          wide_character == 0xE9, "mbrtowc 2 byte (e-acute)");
    check(mbrtowc(&wide_character, "\xE2\x82\xAC", 3, &wide_state) == 3 &&
          wide_character == 0x20AC, "mbrtowc 3 byte (euro)");
    check(mbrtowc(&wide_character, "\xF0\x9F\x98\x80", 4, &wide_state) == 4 &&
          wide_character == 0x1F600, "mbrtowc 4 byte (emoji)");
    check(mbrtowc(&wide_character, "", 1, &wide_state) == 0,
          "mbrtowc NUL mengembalikan 0");

    memset(&wide_state, 0, sizeof(wide_state));
    check(mbrtowc(&wide_character, "\xC3", 1, &wide_state) == (size_t)-2,
          "mbrtowc sequence belum lengkap == (size_t)-2");
    check(wide_state.count == 1, "mbrtowc menyimpan byte parsial di state");
    check(mbrtowc(&wide_character, "\xA9", 1, &wide_state) == 1 &&
          wide_character == 0xE9, "mbrtowc melanjutkan dari state");
    check(wide_state.count == 0, "state direset setelah karakter lengkap");
    check(mbrtowc(&wide_character, "\xFF", 1, &wide_state) == (size_t)-1 &&
          errno == EILSEQ, "mbrtowc byte invalid == EILSEQ");
    check(mbrtowc(&wide_character, "\xC0\x80", 2, &wide_state) == (size_t)-1 &&
          errno == EILSEQ, "mbrtowc overlong ditolak");
    check(mbrtowc(&wide_character, "\xED\xA0\x80", 3, &wide_state) == (size_t)-1 &&
          errno == EILSEQ, "mbrtowc surrogate UTF-8 ditolak");

    check(MB_CUR_MAX == 4, "MB_CUR_MAX == 4");
    check(isatty(1) == 1, "isatty(1)");
    check(isatty(0) == 1, "isatty(0)");
    int tty_pipe[2];
    check(pipe(tty_pipe) == 0, "pipe untuk isatty");
    errno = 0;
    check(isatty(tty_pipe[0]) == 0 && errno == ENOTTY,
          "isatty(pipe) == 0 dan ENOTTY");
    errno = 0;
    check(close_range((unsigned int)tty_pipe[0],
                      (unsigned int)tty_pipe[1], 1) == -1 &&
          errno == ENOTSUP, "close_range menolak flag belum didukung");
    errno = 0;
    check(close_range((unsigned int)tty_pipe[1],
                      (unsigned int)tty_pipe[0], 0) == -1 &&
          errno == EINVAL, "close_range menolak rentang terbalik");
    check(close_range((unsigned int)tty_pipe[0],
                      (unsigned int)tty_pipe[0], 0) == 0 &&
          close_range((unsigned int)tty_pipe[1],
                      (unsigned int)tty_pipe[1], 0) == 0,
          "close_range menutup descriptor dalam rentang");

    struct termios tty_settings;
    check(tcgetattr(1, &tty_settings) == 0, "tcgetattr(1)");
    check((tty_settings.c_lflag & ICANON) != 0, "termios ICANON aktif");
    check((tty_settings.c_lflag & ECHO) != 0, "termios ECHO aktif");
    check(tty_settings.c_cc[VEOF] == 4, "termios VEOF == 4");
    check(tty_settings.c_cc[VERASE] == 127, "termios VERASE == 127");
    check(tcgetattr(0, &tty_settings) == 0, "tcgetattr(0)");
    struct termios raw_settings = tty_settings;
    cfmakeraw(&raw_settings);
    check((raw_settings.c_lflag & (ICANON | ECHO | ISIG)) == 0,
          "cfmakeraw mematikan ICANON/ECHO/ISIG");
    check((raw_settings.c_oflag & OPOST) == 0, "cfmakeraw mematikan OPOST");
    check(raw_settings.c_cc[VMIN] == 1 && raw_settings.c_cc[VTIME] == 0,
          "cfmakeraw VMIN=1 VTIME=0");
    check(tcsetattr(1, TCSANOW, &raw_settings) == 0, "tcsetattr(raw)");
    struct termios applied_settings;
    check(tcgetattr(1, &applied_settings) == 0, "tcgetattr setelah raw");
    check((applied_settings.c_lflag & ICANON) == 0,
          "mode raw tersimpan di kernel");
    check(tcsetattr(1, TCSANOW, &tty_settings) == 0, "tcsetattr(restore)");
    check(tcgetattr(1, &applied_settings) == 0 &&
          (applied_settings.c_lflag & ICANON) != 0,
          "mode canonical dipulihkan");

    struct winsize window;
    memset(&window, 0, sizeof(window));
    check(ioctl(1, TIOCGWINSZ, &window) == 0, "ioctl(TIOCGWINSZ)");
    check(window.ws_col > 0 && window.ws_row > 0,
          "TIOCGWINSZ kolom dan baris > 0");
    int plain_file = open("/tmp/not-a-tty", O_CREAT | O_RDWR | O_TRUNC, 0644);
    check(plain_file >= 0, "open /tmp/not-a-tty");
    if (plain_file >= 0) {
        struct winsize not_tty;
        errno = 0;
        check(ioctl(plain_file, TIOCGWINSZ, &not_tty) == -1 &&
              errno == ENOTTY, "TIOCGWINSZ pada file biasa == ENOTTY");
        check(isatty(plain_file) == 0, "isatty(file biasa) == 0");
        close(plain_file);
    }


    unsetenv("LC_ALL");
    unsetenv("LC_CTYPE");
    check(setenv("LANG", "en_US.UTF-8", 1) == 0, "setenv(LANG=en_US.UTF-8)");
    char *environment_locale = setlocale(LC_ALL, "");
    check(environment_locale != NULL &&
          strcmp(environment_locale, "C.UTF-8") == 0,
          "setlocale(\"\") mengikuti LANG UTF-8");
    check(setenv("LANG", "C", 1) == 0, "setenv(LANG=C)");
    environment_locale = setlocale(LC_ALL, "");
    check(environment_locale != NULL && strcmp(environment_locale, "C") == 0,
          "setlocale(\"\") tanpa UTF-8 menjadi C");
    check(setlocale(LC_ALL, "C.UTF-8") != NULL, "kembali ke C.UTF-8");

    char wide_bytes[8];
    check(wcrtomb(wide_bytes, L'A', NULL) == 1 && wide_bytes[0] == 'A',
          "wcrtomb ASCII");
    check(wcrtomb(wide_bytes, 0xE9, NULL) == 2 &&
          (unsigned char)wide_bytes[0] == 0xC3 &&
          (unsigned char)wide_bytes[1] == 0xA9, "wcrtomb 2 byte");
    check(wcrtomb(wide_bytes, 0x20AC, NULL) == 3 &&
          (unsigned char)wide_bytes[0] == 0xE2, "wcrtomb 3 byte");
    check(wcrtomb(wide_bytes, 0x1F600, NULL) == 4 &&
          (unsigned char)wide_bytes[0] == 0xF0, "wcrtomb 4 byte");
    check(wcrtomb(wide_bytes, 0xD800, NULL) == (size_t)-1 && errno == EILSEQ,
          "wcrtomb surrogate ditolak");
    check(wcrtomb(wide_bytes, 0x110000, NULL) == (size_t)-1 && errno == EILSEQ,
          "wcrtomb di luar Unicode ditolak");

    memset(&wide_state, 0, sizeof(wide_state));
    check(c32rtomb(wide_bytes, 0x1F600, &wide_state) == 4 &&
          (unsigned char)wide_bytes[0] == 0xF0, "c32rtomb 4 byte");
    check(c16rtomb(wide_bytes, 0xD83D, &wide_state) == 0,
          "c16rtomb high surrogate menunda");
    check(c16rtomb(wide_bytes, 0xDE00, &wide_state) == 4 &&
          (unsigned char)wide_bytes[0] == 0xF0 &&
          (unsigned char)wide_bytes[3] == 0x80, "c16rtomb pasangan surrogate");
    check(c16rtomb(wide_bytes, 0xDE00, &wide_state) == (size_t)-1 &&
          errno == EILSEQ, "c16rtomb low surrogate tanpa high ditolak");

    memset(&wide_state, 0, sizeof(wide_state));
    check(c16rtomb(NULL, 0x20AC, &wide_state) == 1,
          "c16rtomb(NULL) mengembalikan 1");
    check(c16rtomb(wide_bytes, 0x20AC, &wide_state) == 3,
          "c16rtomb setelah reset state");


    check(sync_file >= 0, "open /tmp/fsync-test");
    if (sync_file >= 0) {
        check(write(sync_file, "sync", 4) == 4, "write before fsync");
        check(fsync(sync_file) == 0, "fsync(file)");
        check(fdatasync(sync_file) == 0, "fdatasync(file)");
        close(sync_file);
    }
    check(fsync(1) == -1 && errno == EINVAL, "fsync(console) reports EINVAL");

    pthread_t futex_thread;
    futex_word = 0;
    futex_result = -999;
    check(pthread_create(&futex_thread, NULL, futex_waiter, NULL) == 0,
          "pthread_create(futex waiter)");
    usleep(20000);
    check(futex_word == 0, "futex waiter is blocked");
    futex_word = 1;
    check(oct_futex(&futex_word, FUTEX_WAKE, 1) == 1,
          "futex WAKE wakes one waiter");
    check(pthread_join(futex_thread, NULL) == 0, "pthread_join(futex waiter)");
    check(futex_result == 0, "futex WAIT returns 0 after wake");

    futex_word = 0;
    futex_result = -999;
    futex_timeout_started = 0;
    check(pthread_create(&futex_thread, NULL, futex_timeout_waiter, NULL) == 0,
          "pthread_create(futex timeout waiter)");
    for (int wait = 0; wait < 1000 &&
         __atomic_load_n(&futex_timeout_started, __ATOMIC_ACQUIRE) == 0; ++wait) {
        usleep(1000);
    }
    check(__atomic_load_n(&futex_timeout_started, __ATOMIC_ACQUIRE) != 0,
          "futex timeout waiter entered syscall before timeout interval");
    usleep(150000);
    check(pthread_join(futex_thread, NULL) == 0,
          "pthread_join(futex timeout waiter)");
    if (futex_result != -ETIMEDOUT) {
        printf("     futex timeout returned %d (expected %d)\\n",
               (int)futex_result, -ETIMEDOUT);
    }
    check(futex_result == -ETIMEDOUT,
          "futex WAIT timeout returns ETIMEDOUT");

    struct oct_timespec expired_futex_deadline = {0, 0};
    int64_t expired_bitset_wait = oct_syscall6(
        94, (uint64_t)(uintptr_t)&futex_word,
        FUTEX_WAIT_BITSET | FUTEX_CLOCK_REALTIME,
        futex_word, (uint64_t)(uintptr_t)&expired_futex_deadline, 0, 0x1);
    check(expired_bitset_wait == -ETIMEDOUT,
          "futex WAIT_BITSET honors expired absolute realtime deadline");
    check(oct_syscall6(
              94, (uint64_t)(uintptr_t)&futex_word,
              FUTEX_WAKE_BITSET, 1, 0, 0, 0) == -EINVAL,
          "futex WAKE_BITSET rejects an empty bitset");

    futex_bitset_word = 0;
    futex_bitset_result = -999;
    check(pthread_create(&futex_thread, NULL, futex_bitset_waiter, NULL) == 0,
          "pthread_create(futex bitset waiter)");
    usleep(20000);
    futex_bitset_word = 1;
    check(oct_syscall6(
              94, (uint64_t)(uintptr_t)&futex_bitset_word,
              FUTEX_WAKE_BITSET, 1, 0, 0, 0x1) == 0,
          "futex WAKE_BITSET does not wake a nonmatching waiter");
    check(oct_syscall6(
              94, (uint64_t)(uintptr_t)&futex_bitset_word,
              FUTEX_WAKE_BITSET, 1, 0, 0, 0x2) == 1,
          "futex WAKE_BITSET wakes an intersecting waiter");
    check(pthread_join(futex_thread, NULL) == 0,
          "pthread_join(futex bitset waiter)");
    check(futex_bitset_result == 0,
          "futex WAIT_BITSET returns 0 after matching wake");

    futex_requeue_source = 0;
    futex_requeue_target = 0;
    futex_requeue_result = -999;
    check(pthread_create(&futex_thread, NULL, futex_requeue_waiter, NULL) == 0,
          "pthread_create(futex requeue waiter)");
    usleep(20000);
    futex_requeue_source = 1;
    check(oct_syscall6(
              94, (uint64_t)(uintptr_t)&futex_requeue_source,
              FUTEX_REQUEUE, 0, 1,
              (uint64_t)(uintptr_t)&futex_requeue_target, 0) == 0,
          "futex REQUEUE moves waiter without waking it");
    futex_requeue_target = 1;
    check(oct_futex(&futex_requeue_target, FUTEX_WAKE, 1) == 1,
          "futex WAKE releases requeued waiter at destination");
    check(pthread_join(futex_thread, NULL) == 0,
          "pthread_join(futex requeue waiter)");
    check(futex_requeue_result == 0,
          "requeued FUTEX_WAIT resumes successfully");

    counter_value = 0;
    pthread_t counter_a;
    pthread_t counter_b;
    check(pthread_create(&counter_a, NULL, counter_worker, NULL) == 0,
          "pthread_create(counter A)");
    check(pthread_create(&counter_b, NULL, counter_worker, NULL) == 0,
          "pthread_create(counter B)");
    check(pthread_join(counter_a, NULL) == 0, "join counter A");
    check(pthread_join(counter_b, NULL) == 0, "join counter B");
    check(counter_value == 1000, "mutex menjaga shared counter == 1000");

    cond_flag = 0;
    cond_woken = 0;
    pthread_t cond_thread;
    check(pthread_create(&cond_thread, NULL, cond_worker, NULL) == 0,
          "pthread_create(cond waiter)");
    usleep(20000);
    check(cond_woken == 0, "cond waiter masih terblokir");
    pthread_mutex_lock(&cond_mutex);
    cond_flag = 1;
    check(pthread_cond_signal(&cond_variable) == 0, "pthread_cond_signal");
    pthread_mutex_unlock(&cond_mutex);
    check(pthread_join(cond_thread, NULL) == 0, "join cond waiter");
    check(cond_woken == 1, "cond wait dibangunkan signal");

    cond_woken = 0;
    cond_flag = 0;
    pthread_t cond_a;
    pthread_t cond_b;
    check(pthread_create(&cond_a, NULL, cond_worker, NULL) == 0,
          "pthread_create(cond A)");
    check(pthread_create(&cond_b, NULL, cond_worker, NULL) == 0,
          "pthread_create(cond B)");
    usleep(20000);
    pthread_mutex_lock(&cond_mutex);
    cond_flag = 1;
    check(pthread_cond_broadcast(&cond_variable) == 0,
          "pthread_cond_broadcast");
    pthread_mutex_unlock(&cond_mutex);
    check(pthread_join(cond_a, NULL) == 0, "join cond A");
    check(pthread_join(cond_b, NULL) == 0, "join cond B");
    check(cond_woken == 2, "broadcast membangunkan dua waiter");

    struct timespec cond_deadline;
    clock_gettime(CLOCK_REALTIME, &cond_deadline);
    cond_deadline.tv_nsec += 50000000;
    while (cond_deadline.tv_nsec >= 1000000000) {
        cond_deadline.tv_nsec -= 1000000000;
        ++cond_deadline.tv_sec;
    }
    pthread_mutex_lock(&cond_mutex);
    uint64_t cond_started = demo_monotonic_ms();
    int cond_status = pthread_cond_timedwait(&cond_variable, &cond_mutex,
                                             &cond_deadline);
    uint64_t cond_elapsed = demo_monotonic_ms() - cond_started;
    pthread_mutex_unlock(&cond_mutex);
    check(cond_status == ETIMEDOUT, "cond timedwait mengembalikan ETIMEDOUT");
    check(cond_elapsed >= 40 && cond_elapsed < 2000,
          "cond timedwait menunggu sekitar 50ms");

    background_counter = 0;
    background_stop = 0;
    pthread_t background_thread;
    check(pthread_create(&background_thread, NULL, background_worker, NULL) == 0,
          "pthread_create(background worker)");
    usleep(20000);
    unsigned long background_before = background_counter;
    struct timespec progress_deadline;
    clock_gettime(CLOCK_REALTIME, &progress_deadline);
    progress_deadline.tv_nsec += 50000000;
    while (progress_deadline.tv_nsec >= 1000000000) {
        progress_deadline.tv_nsec -= 1000000000;
        ++progress_deadline.tv_sec;
    }
    pthread_mutex_lock(&cond_mutex);
    uint64_t progress_started = demo_monotonic_ms();
    int progress_status = pthread_cond_timedwait(&cond_variable, &cond_mutex,
                                                 &progress_deadline);
    uint64_t progress_elapsed = demo_monotonic_ms() - progress_started;
    pthread_mutex_unlock(&cond_mutex);
    unsigned long background_after = background_counter;
    background_stop = 1;
    check(pthread_join(background_thread, NULL) == 0, "join background worker");
    check(progress_status == ETIMEDOUT,
          "cond timedwait dengan thread lain == ETIMEDOUT");
    check(progress_elapsed >= 40,
          "cond timedwait dengan thread lain menunggu deadline");
    check(background_after > background_before,
          "thread lain tetap jalan selama cond_timedwait");

    check(sem_init(&demo_semaphore, 0, 0) == 0, "sem_init()");
    semaphore_result = 0;
    pthread_t sem_thread;
    check(pthread_create(&sem_thread, NULL, semaphore_worker, NULL) == 0,
          "pthread_create(sem waiter)");
    usleep(20000);
    check(semaphore_result == 0, "sem_wait masih terblokir");
    check(sem_post(&demo_semaphore) == 0, "sem_post()");
    check(pthread_join(sem_thread, NULL) == 0, "join sem waiter");
    check(semaphore_result == 1, "sem_wait dibangunkan sem_post");
    int sem_value = -1;
    check(sem_getvalue(&demo_semaphore, &sem_value) == 0 && sem_value == 0,
          "sem_getvalue() == 0");
    check(sem_trywait(&demo_semaphore) == -1 && errno == EAGAIN,
          "sem_trywait kosong mengembalikan EAGAIN");

    struct timespec sem_deadline;
    clock_gettime(CLOCK_REALTIME, &sem_deadline);
    sem_deadline.tv_nsec += 50000000;
    while (sem_deadline.tv_nsec >= 1000000000) {
        sem_deadline.tv_nsec -= 1000000000;
        ++sem_deadline.tv_sec;
    }
    uint64_t sem_started = demo_monotonic_ms();
    int sem_status = sem_timedwait(&demo_semaphore, &sem_deadline);
    uint64_t sem_elapsed = demo_monotonic_ms() - sem_started;
    check(sem_status == -1 && errno == ETIMEDOUT,
          "sem_timedwait mengembalikan ETIMEDOUT");
    check(sem_elapsed >= 40 && sem_elapsed < 2000,
          "sem_timedwait menunggu sekitar 50ms");
    check(sem_destroy(&demo_semaphore) == 0, "sem_destroy()");

    int epoll_pipe[2];
    check(pipe(epoll_pipe) == 0, "pipe for epoll");
    int epoll_fd = epoll_create1(0);
    check(epoll_fd >= 0, "epoll_create1()");
    struct epoll_event interest;
    interest.events = EPOLLIN;
    interest.data = 0x7;
    check(epoll_ctl(epoll_fd, EPOLL_CTL_ADD, epoll_pipe[0], &interest) == 0,
          "epoll_ctl(ADD)");
    check(epoll_ctl(epoll_fd, EPOLL_CTL_ADD, epoll_pipe[0], &interest) == -1 &&
              errno == EEXIST,
          "epoll_ctl(ADD) duplikat mengembalikan EEXIST");
    struct epoll_event ready_events[2];
    check(epoll_wait(epoll_fd, ready_events, 2, 0) == 0,
          "epoll_wait tanpa readiness mengembalikan 0");
    check(write(epoll_pipe[1], "e", 1) == 1, "write ke pipe epoll");
    int ready_count = epoll_wait(epoll_fd, ready_events, 2, 50);
    check(ready_count == 1, "epoll_wait melaporkan pipe siap");
    check((ready_events[0].events & EPOLLIN) != 0 &&
              ready_events[0].data == 0x7,
          "epoll event membawa mask dan data");
    interest.events = EPOLLOUT;
    interest.data = 0x9;
    check(epoll_ctl(epoll_fd, EPOLL_CTL_MOD, epoll_pipe[0], &interest) == 0,
          "epoll_ctl(MOD)");
    check(epoll_ctl(epoll_fd, EPOLL_CTL_DEL, epoll_pipe[0], NULL) == 0,
          "epoll_ctl(DEL)");
    check(epoll_ctl(epoll_fd, EPOLL_CTL_DEL, epoll_pipe[0], NULL) == -1 &&
              errno == ENOENT,
          "epoll_ctl(DEL) tanpa entri mengembalikan ENOENT");
    close(epoll_pipe[0]);
    close(epoll_pipe[1]);
    close(epoll_fd);

    int event_fd = eventfd(0, 0);
    check(event_fd >= 0, "eventfd()");
    uint64_t event_counter = 5;
    check(write(event_fd, &event_counter, sizeof(event_counter)) ==
              (ssize_t)sizeof(event_counter),
          "eventfd write");
    uint64_t event_read = 0;
    check(read(event_fd, &event_read, sizeof(event_read)) ==
              (ssize_t)sizeof(event_read),
          "eventfd read");
    check(event_read == 5, "eventfd mengembalikan akumulasi counter");
    check(oct_read(event_fd, &event_read, sizeof(event_read)) == -EAGAIN,
          "eventfd kosong mengembalikan EAGAIN dari syscall");
    close(event_fd);

    int timer_fd = timerfd_create(CLOCK_MONOTONIC, 0);
    check(timer_fd >= 0, "timerfd_create()");
    uint64_t expirations = 0;
    check(oct_read(timer_fd, &expirations, sizeof(expirations)) == -EAGAIN,
          "timerfd yang belum diarm mengembalikan EAGAIN");
    struct itimerspec timer_spec;
    memset(&timer_spec, 0, sizeof(timer_spec));
    timer_spec.it_value.tv_nsec = 20 * 1000 * 1000;
    check(timerfd_settime(timer_fd, 0, &timer_spec, NULL) == 0,
          "timerfd_settime()");
    int timer_epoll = epoll_create1(0);
    interest.events = EPOLLIN;
    interest.data = 0x42;
    check(epoll_ctl(timer_epoll, EPOLL_CTL_ADD, timer_fd, &interest) == 0,
          "epoll_ctl(timerfd)");
    check(epoll_wait(timer_epoll, ready_events, 1, 1000) == 1,
          "epoll_wait menunggu timerfd");
    check(read(timer_fd, &expirations, sizeof(expirations)) ==
              (ssize_t)sizeof(expirations) &&
              expirations >= 1,
          "timerfd read melaporkan expiration");
    close(timer_epoll);
    close(timer_fd);

    int pair[2];
    check(socketpair(AF_UNIX, SOCK_STREAM, 0, pair) == 0, "socketpair()");
    check(write(pair[0], "ping", 4) == 4, "write socketpair ujung 0");
    char pair_buffer[8] = {0};
    check(read(pair[1], pair_buffer, sizeof(pair_buffer)) == 4,
          "read socketpair ujung 1");
    check(memcmp(pair_buffer, "ping", 4) == 0, "data socketpair sampai");
    check(write(pair[1], "pong", 4) == 4, "write socketpair ujung 1");
    check(read(pair[0], pair_buffer, sizeof(pair_buffer)) == 4 &&
              memcmp(pair_buffer, "pong", 4) == 0,
          "socketpair dua arah");
    close(pair[0]);
    close(pair[1]);

    struct timespec nap;
    nap.tv_sec = 0;
    nap.tv_nsec = 20 * 1000 * 1000;
    check(clock_nanosleep(CLOCK_MONOTONIC, 0, &nap, NULL) == 0,
          "clock_nanosleep relatif");
    struct timespec absolute;
    check(clock_gettime(CLOCK_MONOTONIC, &absolute) == 0,
          "clock_gettime sebelum absolute sleep");
    absolute.tv_nsec += 20 * 1000 * 1000;
    if (absolute.tv_nsec >= 1000000000) {
        absolute.tv_sec += 1;
        absolute.tv_nsec -= 1000000000;
    }
    check(clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &absolute, NULL) == 0,
          "clock_nanosleep absolut");

    int cloexec_source = open("/tmp/fcntl-cloexec-source",
                               O_CREAT | O_RDWR | O_TRUNC, 0600);
    check(cloexec_source >= 0, "open source for F_DUPFD_CLOEXEC");
    if (cloexec_source >= 0) {
        int duplicate_plain = fcntl(cloexec_source, F_DUPFD, 0);
        check(duplicate_plain >= 0, "fcntl(F_DUPFD) duplicates descriptor");
        if (duplicate_plain >= 0) {
            check((fcntl(duplicate_plain, F_GETFD) & FD_CLOEXEC) == 0,
                  "F_DUPFD clears FD_CLOEXEC on duplicate");
            close(duplicate_plain);
        }
        int duplicate_cloexec = fcntl(cloexec_source, F_DUPFD_CLOEXEC, 0);
        check(duplicate_cloexec >= 0,
              "fcntl(F_DUPFD_CLOEXEC) duplicates descriptor");
        if (duplicate_cloexec >= 0) {
            check((fcntl(duplicate_cloexec, F_GETFD) & FD_CLOEXEC) != 0,
                  "F_DUPFD_CLOEXEC sets FD_CLOEXEC");
            close(duplicate_cloexec);
        }
        close(cloexec_source);
        unlink("/tmp/fcntl-cloexec-source");
    }

    int nonblock_pipe[2];
    check(pipe(nonblock_pipe) == 0, "pipe untuk O_NONBLOCK");
    int status_flags = fcntl(nonblock_pipe[0], F_GETFL);
    check(status_flags >= 0 && (status_flags & O_NONBLOCK) == 0,
          "pipe awalnya blocking");
    check(fcntl(nonblock_pipe[0], F_SETFL, status_flags | O_NONBLOCK) == 0,
          "fcntl(F_SETFL, O_NONBLOCK)");
    status_flags = fcntl(nonblock_pipe[0], F_GETFL);
    check(status_flags >= 0 && (status_flags & O_NONBLOCK) != 0,
          "F_GETFL melaporkan O_NONBLOCK");
    char nonblock_byte = 0;
    check(read(nonblock_pipe[0], &nonblock_byte, 1) == -1 && errno == EAGAIN,
          "read pipe nonblocking langsung EAGAIN");
    check(write(nonblock_pipe[1], "n", 1) == 1, "write ke pipe nonblocking");
    check(read(nonblock_pipe[0], &nonblock_byte, 1) == 1 && nonblock_byte == 'n',
          "read pipe nonblocking mengembalikan data");
    close(nonblock_pipe[0]);
    close(nonblock_pipe[1]);

    int pipe2_fds[2];
    check(pipe2(pipe2_fds, O_NONBLOCK) == 0, "pipe2(O_NONBLOCK)");
    char pipe2_byte = 0;
    check(read(pipe2_fds[0], &pipe2_byte, 1) == -1 && errno == EAGAIN,
          "pipe2 nonblocking read EAGAIN");
    close(pipe2_fds[0]);
    close(pipe2_fds[1]);

    int edge_pipe[2];
    check(pipe(edge_pipe) == 0, "pipe untuk epoll edge");
    int edge_epoll = epoll_create1(0);
    interest.events = EPOLLIN | EPOLLET;
    interest.data = 0x11;
    check(epoll_ctl(edge_epoll, EPOLL_CTL_ADD, edge_pipe[0], &interest) == 0,
          "epoll_ctl(ADD, EPOLLET)");
    check(write(edge_pipe[1], "aa", 2) == 2, "write edge pertama");
    check(epoll_wait(edge_epoll, ready_events, 1, 50) == 1,
          "EPOLLET melaporkan edge pertama");
    check(epoll_wait(edge_epoll, ready_events, 1, 20) == 0,
          "EPOLLET tidak mengulang tanpa edge baru");
    char edge_buffer[4];
    check(read(edge_pipe[0], edge_buffer, 1) == 1, "read satu byte edge");
    check(epoll_wait(edge_epoll, ready_events, 1, 20) == 0,
          "EPOLLET diam setelah read parsial");
    check(read(edge_pipe[0], edge_buffer, 1) == 1, "drain byte edge terakhir");
    check(epoll_wait(edge_epoll, ready_events, 1, 20) == 0,
          "EPOLLET diam setelah pipe kosong");
    check(write(edge_pipe[1], "b", 1) == 1, "write edge kedua");
    check(epoll_wait(edge_epoll, ready_events, 1, 50) == 1,
          "EPOLLET melaporkan edge baru");
    close(edge_pipe[0]);
    close(edge_pipe[1]);
    close(edge_epoll);

    int oneshot_pipe[2];
    check(pipe(oneshot_pipe) == 0, "pipe untuk epoll oneshot");
    int oneshot_epoll = epoll_create1(0);
    interest.events = EPOLLIN | EPOLLONESHOT;
    interest.data = 0x22;
    check(epoll_ctl(oneshot_epoll, EPOLL_CTL_ADD, oneshot_pipe[0], &interest) == 0,
          "epoll_ctl(ADD, EPOLLONESHOT)");
    check(write(oneshot_pipe[1], "x", 1) == 1, "write oneshot");
    check(epoll_wait(oneshot_epoll, ready_events, 1, 50) == 1,
          "EPOLLONESHOT melaporkan sekali");
    check(epoll_wait(oneshot_epoll, ready_events, 1, 20) == 0,
          "EPOLLONESHOT tidak mengulang");
    check(epoll_ctl(oneshot_epoll, EPOLL_CTL_MOD, oneshot_pipe[0], &interest) == 0,
          "epoll_ctl(MOD) mengarm ulang ONESHOT");
    check(epoll_wait(oneshot_epoll, ready_events, 1, 50) == 1,
          "EPOLLONESHOT aktif lagi setelah MOD");
    close(oneshot_pipe[0]);
    close(oneshot_pipe[1]);
    close(oneshot_epoll);

    struct sigaction action;
    memset(&action, 0, sizeof(action));
    action.sa_handler = user_signal_handler;
    sigemptyset(&action.sa_mask);
    check(sigaction(SIGUSR1, &action, NULL) == 0, "sigaction(SIGUSR1)");
    check(raise(SIGUSR1) == 0, "raise(SIGUSR1)");
    check(usr1_count == 1, "handler SIGUSR1 jalan lalu sigreturn memulihkan");

    struct sigaction previous_action;
    check(sigaction(SIGUSR1, NULL, &previous_action) == 0 &&
              previous_action.sa_handler == user_signal_handler,
          "sigaction(NULL) mengembalikan handler lama");

    sighandler_t previous_handler = signal(SIGUSR1, SIG_IGN);
    check(previous_handler == user_signal_handler,
          "signal() mengembalikan handler sebelumnya");
    check(raise(SIGUSR1) == 0 && usr1_count == 1, "SIG_IGN menahan handler");
    check(signal(SIGUSR1, user_signal_handler) == SIG_IGN,
          "signal() memulihkan handler");

    check(sigaction(SIGUSR2, &action, NULL) == 0, "sigaction(SIGUSR2)");

    sigset_t block_mask;
    sigset_t old_mask;
    sigset_t pending_mask;
    sigemptyset(&block_mask);
    sigaddset(&block_mask, SIGUSR2);
    check(sigprocmask(SIG_BLOCK, &block_mask, &old_mask) == 0,
          "sigprocmask(SIG_BLOCK)");
    check(raise(SIGUSR2) == 0 && usr2_count == 0,
          "signal terblokir belum dijalankan");
    check(sigpending(&pending_mask) == 0 &&
              sigismember(&pending_mask, SIGUSR2),
          "sigpending mencatat SIGUSR2");
    check(sigprocmask(SIG_SETMASK, &old_mask, NULL) == 0,
          "sigprocmask(SIG_SETMASK)");
    check(usr2_count == 1, "handler jalan tepat setelah unblock");

    check(kill(99999, SIGUSR1) == -1 && errno == ESRCH,
          "kill ke pid tak ada mengembalikan ESRCH");
    check(sigaction(SIGKILL, &action, NULL) == -1 && errno == EINVAL,
          "sigaction(SIGKILL) ditolak");

    check(pipe(interrupt_pipe) == 0, "pipe untuk uji EINTR");
    pthread_t interrupt_thread;

    interrupt_result = -999;
    interrupt_errno = 0;
    check(pthread_create(&interrupt_thread, NULL, blocked_pipe_reader, NULL) == 0,
          "pthread_create(reader terblokir)");
    usleep(20000);
    check(kill(getpid(), SIGUSR1) == 0, "kill membangunkan read");
    check(pthread_join(interrupt_thread, NULL) == 0, "join reader");
    check(interrupt_result == -1 && interrupt_errno == EINTR,
          "read terblokir mengembalikan EINTR");

    interrupt_result = -999;
    interrupt_errno = 0;
    check(pthread_create(&interrupt_thread, NULL, blocked_pipe_poller, NULL) == 0,
          "pthread_create(poller terblokir)");
    usleep(20000);
    check(kill(getpid(), SIGUSR1) == 0, "kill membangunkan poll");
    check(pthread_join(interrupt_thread, NULL) == 0, "join poller");
    check(interrupt_result == -1 && interrupt_errno == EINTR,
          "poll terblokir mengembalikan EINTR");

    interrupt_result = -999;
    interrupt_errno = 0;
    check(pthread_create(&interrupt_thread, NULL, blocked_pipe_selector, NULL) == 0,
          "pthread_create(selector terblokir)");
    usleep(20000);
    check(kill(getpid(), SIGUSR1) == 0, "kill membangunkan select");
    check(pthread_join(interrupt_thread, NULL) == 0, "join selector");
    check(interrupt_result == -1 && interrupt_errno == EINTR,
          "select terblokir mengembalikan EINTR");

    interrupt_futex_word = 0;
    interrupt_result = -999;
    interrupt_errno = 0;
    check(pthread_create(&interrupt_thread, NULL, blocked_futex_waiter, NULL) == 0,
          "pthread_create(futex waiter terblokir)");
    usleep(20000);
    check(kill(getpid(), SIGUSR1) == 0, "kill membangunkan futex");
    check(pthread_join(interrupt_thread, NULL) == 0, "join futex waiter");
    check(interrupt_result == -EINTR, "futex WAIT terblokir mengembalikan EINTR");

    action.sa_handler = user_signal_handler;
    action.sa_flags = SA_RESTART;
    sigemptyset(&action.sa_mask);
    check(sigaction(SIGUSR2, &action, NULL) == 0,
          "sigaction(SIGUSR2, SA_RESTART)");
    int usr2_before = usr2_count;
    interrupt_result = -999;
    interrupt_errno = 0;
    check(pthread_create(&interrupt_thread, NULL, blocked_pipe_reader, NULL) == 0,
          "pthread_create(reader SA_RESTART)");
    usleep(20000);
    check(kill(getpid(), SIGUSR2) == 0, "kill dengan SA_RESTART");
    usleep(30000);
    check(write(interrupt_pipe[1], "r", 1) == 1, "write setelah restart");
    check(pthread_join(interrupt_thread, NULL) == 0, "join reader restart");
    check(usr2_count == usr2_before + 1, "handler SA_RESTART dijalankan");
    check(interrupt_result == 1, "read diulang otomatis (SA_RESTART)");
    close(interrupt_pipe[0]);
    close(interrupt_pipe[1]);

    void *before = sbrk(0);
    check(before != (void *)-1, "sbrk(0)");
    void *block = sbrk(4096);
    check(block == before, "sbrk(4096) returns previous break");
    ((char *)block)[0] = 'x';
    ((char *)block)[4095] = 'y';
    check(sbrk(0) == (char *)block + 4096, "sbrk advanced by one page");

    int pipe_fds[2];
    check(pipe(pipe_fds) == 0, "pipe()");

    struct pollfd read_end;
    read_end.fd = pipe_fds[0];
    read_end.events = POLLIN;
    read_end.revents = 0;
    check(poll(&read_end, 1, 0) == 0 && read_end.revents == 0,
          "poll empty pipe is not ready");

    fd_set reads;
    FD_ZERO(&reads);
    FD_SET(pipe_fds[0], &reads);
    struct timeval immediate = {0, 0};
    check(select(pipe_fds[0] + 1, &reads, 0, 0, &immediate) == 0 &&
              !FD_ISSET(pipe_fds[0], &reads),
          "select empty pipe is not ready");

    check(write(pipe_fds[1], "hello", 5) == 5, "write pipe");
    read_end.revents = 0;
    check(poll(&read_end, 1, 0) == 1, "poll pipe ready after write");
    check((read_end.revents & POLLIN) != 0, "poll reports POLLIN");

    FD_ZERO(&reads);
    FD_SET(pipe_fds[0], &reads);
    check(select(pipe_fds[0] + 1, &reads, 0, 0, &immediate) == 1,
          "select pipe ready after write");
    check(FD_ISSET(pipe_fds[0], &reads), "select sets the fd bit");

    char buffer[8] = {0};
    check(read(pipe_fds[0], buffer, 5) == 5 && memcmp(buffer, "hello", 5) == 0,
          "read pipe data");

    check(close(pipe_fds[1]) == 0, "close write end");
    read_end.fd = pipe_fds[0];
    read_end.events = POLLIN;
    read_end.revents = 0;
    check(poll(&read_end, 1, 0) == 1 && (read_end.revents & POLLHUP) != 0,
          "poll reports POLLHUP at EOF");
    close(pipe_fds[0]);

    struct pollfd input;
    input.fd = 0;
    input.events = POLLIN;
    input.revents = 0;
    printf("type a line, then press Enter: ");
    errno = 0;
    time_t started = time(NULL);
    int poll_result = poll(&input, 1, 10000);
    time_t finished = time(NULL);
    printf("     poll(stdin) returned %d revents=0x%x errno=%d after %llds\n",
           poll_result, (unsigned int)(unsigned short)input.revents, errno,
           (long long)(finished - started));
    check(poll_result == 1 && (input.revents & POLLIN) != 0,
          "poll stdin ready after a typed line");

    char line[64];
    ssize_t count = read(0, line, sizeof(line) - 1);
    if (count < 0) {
        count = 0;
    }
    line[count] = '\0';
    printf("read %lld bytes: %s", (long long)count, line);
    check(count == 6, "read returns the typed line");

    FD_ZERO(&reads);
    FD_SET(0, &reads);
    struct timeval quiet = {2, 0};
    check(select(1, &reads, 0, 0, &quiet) == 0,
          "select stdin quiet after the line was consumed");

    if (failures == 0) {
        printf("SYSCALL-TEST: PASS\n");
    } else {
        printf("SYSCALL-TEST: FAIL(%d)\n", failures);
    }
    return failures == 0 ? 0 : 1;
}
