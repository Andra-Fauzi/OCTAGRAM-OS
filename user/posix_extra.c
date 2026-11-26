/*
 * POSIX/GNU compatibility routines for OCTAGRAM's C library.
 *
 * These live in their own translation unit because they only need the public
 * API (malloc, opendir/readdir, stat, open, write, clock_gettime, uname). The
 * few functions that need allocator internals stay in runtime.c.
 *
 * The goal is what an LLVM- or rustc-based toolchain actually calls: pattern
 * matching, directory-glob expansion, line-oriented reading, temporary
 * directories, affinity queries, and the BSD string helpers.
 */

#include <ctype.h>
#include <dirent.h>
#include <err.h>
#include <errno.h>
#include <error.h>
#include <fnmatch.h>
#include <getopt.h>
#include <fcntl.h>
#include <fts.h>
#include <limits.h>
#include <linux/futex.h>
#include <malloc.h>
#include <ftw.h>
#include <glob.h>
#include <pthread.h>
#include <sched.h>
#include <search.h>
#include <semaphore.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>
#include <sys/mman.h>
#include <sys/random.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/sysinfo.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include <wordexp.h>

/* ---- String helpers ----------------------------------------------------- */

void *memmem(const void *haystack, size_t haystack_length, const void *needle,
             size_t needle_length)
{
    if (needle_length == 0) {
        return (void *)haystack;
    }
    if (haystack == NULL || needle == NULL || needle_length > haystack_length) {
        return NULL;
    }
    const unsigned char *base = haystack;
    const unsigned char *pattern = needle;
    size_t last = haystack_length - needle_length;
    for (size_t offset = 0; offset <= last; offset++) {
        if (base[offset] == pattern[0] &&
            memcmp(base + offset, pattern, needle_length) == 0) {
            return (void *)(base + offset);
        }
    }
    return NULL;
}

char *strsep(char **string_pointer, const char *delimiters)
{
    if (string_pointer == NULL || *string_pointer == NULL) {
        return NULL;
    }
    char *begin = *string_pointer;
    char *cursor = begin;
    while (*cursor != '\0' && strchr(delimiters, *cursor) == NULL) {
        cursor++;
    }
    if (*cursor == '\0') {
        *string_pointer = NULL;
    } else {
        *cursor = '\0';
        *string_pointer = cursor + 1;
    }
    return begin;
}

size_t strnlen(const char *text, size_t maximum)
{
    size_t length = 0;
    if (text == NULL) {
        return 0;
    }
    while (length < maximum && text[length] != '\0') {
        length++;
    }
    return length;
}

size_t strlcpy(char *destination, const char *source, size_t size)
{
    size_t source_length = strlen(source);
    if (size != 0) {
        size_t copied = source_length < size - 1 ? source_length : size - 1;
        memcpy(destination, source, copied);
        destination[copied] = '\0';
    }
    return source_length;
}

size_t strlcat(char *destination, const char *source, size_t size)
{
    size_t destination_length = strnlen(destination, size);
    size_t source_length = strlen(source);
    if (destination_length == size) {
        return size + source_length;
    }
    size_t room = size - destination_length - 1;
    size_t copied = source_length < room ? source_length : room;
    memcpy(destination + destination_length, source, copied);
    destination[destination_length + copied] = '\0';
    return destination_length + source_length;
}

void explicit_bzero(void *buffer, size_t length)
{
    volatile unsigned char *bytes = buffer;
    if (buffer == NULL) {
        return;
    }
    for (size_t index = 0; index < length; index++) {
        bytes[index] = 0;
    }
}

void *reallocarray(void *pointer, size_t count, size_t size)
{
    if (size != 0 && count > SIZE_MAX / size) {
        errno = ENOMEM;
        return NULL;
    }
    return realloc(pointer, count * size);
}

/* ---- qsort_r ------------------------------------------------------------ */

struct oct_qsort_frame {
    unsigned char *base;
    size_t size;
    int (*compare)(const void *, const void *, void *);
    void *argument;
};

static void oct_qsort_swap(unsigned char *left, unsigned char *right, size_t size)
{
    while (size-- != 0) {
        unsigned char temporary = *left;
        *left++ = *right;
        *right++ = temporary;
    }
}

static void oct_qsort_range(struct oct_qsort_frame *frame, size_t low, size_t high)
{
    /* Insertion sort below a small threshold, median-of-three quicksort above. */
    while (high - low > 12) {
        size_t middle = low + (high - low) / 2;
        if (frame->compare(frame->base + middle * frame->size,
                           frame->base + low * frame->size, frame->argument) < 0) {
            oct_qsort_swap(frame->base + middle * frame->size,
                           frame->base + low * frame->size, frame->size);
        }
        if (frame->compare(frame->base + high * frame->size,
                           frame->base + low * frame->size, frame->argument) < 0) {
            oct_qsort_swap(frame->base + high * frame->size,
                           frame->base + low * frame->size, frame->size);
        }
        if (frame->compare(frame->base + high * frame->size,
                           frame->base + middle * frame->size, frame->argument) < 0) {
            oct_qsort_swap(frame->base + high * frame->size,
                           frame->base + middle * frame->size, frame->size);
        }
        oct_qsort_swap(frame->base + middle * frame->size,
                       frame->base + high * frame->size, frame->size);

        size_t pivot = high;
        size_t store = low;
        for (size_t index = low; index < high; index++) {
            if (frame->compare(frame->base + index * frame->size,
                               frame->base + pivot * frame->size,
                               frame->argument) < 0) {
                oct_qsort_swap(frame->base + index * frame->size,
                               frame->base + store * frame->size, frame->size);
                store++;
            }
        }
        oct_qsort_swap(frame->base + store * frame->size,
                       frame->base + pivot * frame->size, frame->size);

        /* Recurse into the smaller half, iterate over the larger one. */
        if (store == 0 || store - low < high - store) {
            if (store > low) {
                oct_qsort_range(frame, low, store - 1);
            }
            low = store + 1;
        } else {
            if (store < high) {
                oct_qsort_range(frame, store + 1, high);
            }
            if (store == 0) {
                break;
            }
            high = store - 1;
        }
    }

    for (size_t index = low + 1; index <= high && high != 0; index++) {
        size_t position = index;
        while (position > low &&
               frame->compare(frame->base + (position - 1) * frame->size,
                              frame->base + position * frame->size,
                              frame->argument) > 0) {
            oct_qsort_swap(frame->base + (position - 1) * frame->size,
                           frame->base + position * frame->size, frame->size);
            position--;
        }
    }
}

void qsort_r(void *base, size_t count, size_t size,
             int (*compare)(const void *, const void *, void *), void *argument)
{
    if (base == NULL || compare == NULL || size == 0 || count < 2) {
        return;
    }
    struct oct_qsort_frame frame = {base, size, compare, argument};
    oct_qsort_range(&frame, 0, count - 1);
}

/* ---- fnmatch ------------------------------------------------------------ */

static int oct_match_class(const char *pattern, size_t *consumed, char value)
{
    /* pattern points at '['; *consumed receives the length including ']'. */
    size_t index = 1;
    int negate = 0;
    int matched = 0;
    if (pattern[index] == '!' || pattern[index] == '^') {
        negate = 1;
        index++;
    }
    do {
        if (pattern[index] == '\0') {
            return -1; /* unterminated class */
        }
        char low = pattern[index];
        if (low == ']' && index > (size_t)(negate ? 2 : 1)) {
            break;
        }
        if (pattern[index + 1] == '-' && pattern[index + 2] != ']' &&
            pattern[index + 2] != '\0') {
            char high = pattern[index + 2];
            if (value >= low && value <= high) {
                matched = 1;
            }
            index += 3;
        } else {
            if (value == low) {
                matched = 1;
            }
            index++;
        }
    } while (1);
    *consumed = index + 1;
    return negate ? !matched : matched;
}

static int oct_fnmatch_run(const char *pattern, const char *string, int flags,
                           int component_start)
{
    if (*pattern == '\0') {
        return *string == '\0' ? 0 : FNM_NOMATCH;
    }

    /* A leading '.' only matches a literal '.' when FNM_PERIOD is set. */
    if ((flags & FNM_PERIOD) != 0 && component_start && *string == '.' &&
        *pattern != '.') {
        return FNM_NOMATCH;
    }

    switch (*pattern) {
    case '*': {
        while (*pattern == '*') {
            pattern++;
        }
        if (*pattern == '\0') {
            if ((flags & FNM_PATHNAME) != 0 && strchr(string, '/') != NULL) {
                return FNM_NOMATCH;
            }
            return 0;
        }
        for (const char *candidate = string;; candidate++) {
            if (oct_fnmatch_run(pattern, candidate, flags, component_start) == 0) {
                return 0;
            }
            if (*candidate == '\0') {
                return FNM_NOMATCH;
            }
            if ((flags & FNM_PATHNAME) != 0 && *candidate == '/') {
                return FNM_NOMATCH;
            }
            component_start = 0;
        }
    }
    case '?':
        if (*string == '\0') {
            return FNM_NOMATCH;
        }
        if ((flags & FNM_PATHNAME) != 0 && *string == '/') {
            return FNM_NOMATCH;
        }
        return oct_fnmatch_run(pattern + 1, string + 1, flags, 0);
    case '[': {
        if (*string == '\0') {
            return FNM_NOMATCH;
        }
        size_t consumed = 0;
        int result = oct_match_class(pattern, &consumed, *string);
        if (result < 0) {
            /* An unterminated class matches a literal '['. */
            if (*string != '[') {
                return FNM_NOMATCH;
            }
            return oct_fnmatch_run(pattern + 1, string + 1, flags, 0);
        }
        if (result == 0) {
            return FNM_NOMATCH;
        }
        return oct_fnmatch_run(pattern + consumed, string + 1, flags, 0);
    }
    case '\\':
        if ((flags & FNM_NOESCAPE) == 0 && pattern[1] != '\0') {
            pattern++;
        }
        /* fall through */
    default:
        if (*string != *pattern) {
            return FNM_NOMATCH;
        }
        return oct_fnmatch_run(pattern + 1, string + 1, flags,
                               *pattern == '/' ? 1 : 0);
    }
}

int fnmatch(const char *pattern, const char *string, int flags)
{
    if (pattern == NULL || string == NULL) {
        return FNM_NOMATCH;
    }
    return oct_fnmatch_run(pattern, string, flags, 1);
}

/* ---- glob --------------------------------------------------------------- */

static void oct_glob_free_entries(glob_t *result, size_t from)
{
    for (size_t index = from; index < result->gl_pathc; index++) {
        free(result->gl_pathv[index]);
        result->gl_pathv[index] = NULL;
    }
}

static int oct_glob_append(glob_t *result, const char *path)
{
    char **grown = realloc(result->gl_pathv,
                           (result->gl_pathc + 1) * sizeof(char *));
    if (grown == NULL) {
        return GLOB_NOSPACE;
    }
    result->gl_pathv = grown;
    char *copy = malloc(strlen(path) + 2);
    if (copy == NULL) {
        return GLOB_NOSPACE;
    }
    strcpy(copy, path);
    if ((result->gl_flags & GLOB_MARK) != 0) {
        struct stat information;
        if (stat(copy, &information) == 0 && S_ISDIR(information.st_mode)) {
            size_t length = strlen(copy);
            copy[length] = '/';
            copy[length + 1] = '\0';
        }
    }
    result->gl_pathv[result->gl_pathc++] = copy;
    return 0;
}

static int oct_glob_compare(const void *left, const void *right)
{
    return strcmp(*(const char *const *)left, *(const char *const *)right);
}

static int oct_glob_walk(const char *prefix, const char *pattern, glob_t *result);

static int oct_glob_in_directory(const char *directory, const char *component,
                                 const char *rest, glob_t *result)
{
    DIR *stream = opendir(directory[0] == '\0' ? "." : directory);
    if (stream == NULL) {
        return 0;
    }
    int status = 0;
    struct dirent *entry;
    while ((entry = readdir(stream)) != NULL) {
        puts("OCT-GLOB: directory entry read");
        fflush(stdout);
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
            continue;
        }
        puts("OCT-GLOB: dot check passed");
        fflush(stdout);
        if (fnmatch(component, entry->d_name, 0) != 0) {
            continue;
        }
        puts("OCT-GLOB: fnmatch passed");
        fflush(stdout);
        size_t length = strlen(directory) + strlen(entry->d_name) + 3;
        char *child = malloc(length);
        if (child == NULL) {
            status = GLOB_NOSPACE;
            break;
        }
        if (directory[0] == '\0') {
            snprintf(child, length, "%s", entry->d_name);
        } else {
            snprintf(child, length, "%s/%s", directory, entry->d_name);
        }
        puts("OCT-GLOB: recurse");
        fflush(stdout);
        status = oct_glob_walk(child, rest, result);
        puts("OCT-GLOB: recurse returned");
        fflush(stdout);
        free(child);
        if (status != 0) {
            break;
        }
    }
    closedir(stream);
    return status;
}

static int oct_glob_walk(const char *prefix, const char *pattern, glob_t *result)
{
    if (pattern == NULL || *pattern == '\0') {
        return oct_glob_append(result, prefix);
    }

    const char *slash = strchr(pattern, '/');
    size_t component_length = slash != NULL ? (size_t)(slash - pattern)
                                             : strlen(pattern);
    char component[256];
    if (component_length >= sizeof(component)) {
        return 0;
    }
    memcpy(component, pattern, component_length);
    component[component_length] = '\0';
    const char *rest = slash != NULL ? slash + 1 : NULL;
    while (rest != NULL && *rest == '/') {
        rest++;
    }

    int magic = strpbrk(component, "*?[\\") != NULL;
    if (!magic) {
        size_t length = strlen(prefix) + component_length + 2;
        char *child = malloc(length);
        if (child == NULL) {
            return GLOB_NOSPACE;
        }
        if (prefix[0] == '\0') {
            snprintf(child, length, "%s", component);
        } else if (strcmp(prefix, "/") == 0) {
            snprintf(child, length, "/%s", component);
        } else {
            snprintf(child, length, "%s/%s", prefix, component);
        }
        struct stat information;
        int status = 0;
        /* An intermediate component must exist; the last one may be reported
         * by GLOB_NOCHECK instead. */
        if (stat(child, &information) == 0) {
            status = oct_glob_walk(child, rest, result);
        } else if (rest == NULL && (result->gl_flags & GLOB_NOCHECK) != 0) {
            status = oct_glob_append(result, prefix[0] == '\0' ? component : child);
        }
        free(child);
        return status;
    }

    return oct_glob_in_directory(prefix, component, rest, result);
}

int glob(const char *pattern, int flags, int (*error_function)(const char *, int),
         glob_t *result)
{
    if (pattern == NULL || result == NULL) {
        return GLOB_ABORTED;
    }
    if ((flags & GLOB_APPEND) == 0) {
        memset(result, 0, sizeof(*result));
        result->gl_offs = 1;
    }
    result->gl_flags = flags;

    size_t existing = result->gl_pathc;
    const char *pattern_start = pattern;
    char *prefix = NULL;
    int status;

    if (pattern[0] == '/') {
        prefix = malloc(2);
        if (prefix == NULL) {
            return GLOB_NOSPACE;
        }
        strcpy(prefix, "/");
        while (*pattern_start == '/') {
            pattern_start++;
        }
    } else {
        prefix = malloc(1);
        if (prefix == NULL) {
            return GLOB_NOSPACE;
        }
        prefix[0] = '\0';
    }

    puts("OCT-GLOB: walk begin");
    fflush(stdout);
    status = oct_glob_walk(prefix, pattern_start, result);
    puts("OCT-GLOB: walk returned");
    fflush(stdout);
    free(prefix);
    if (status != 0) {
        oct_glob_free_entries(result, existing);
        result->gl_pathc = existing;
        return status;
    }

    if (result->gl_pathc == existing) {
        if ((flags & GLOB_NOCHECK) != 0) {
            return oct_glob_append(result, pattern);
        }
        if (error_function != NULL) {
            error_function(pattern, ENOENT);
        }
        return GLOB_NOMATCH;
    }

    if ((flags & GLOB_NOSORT) == 0) {
        puts("OCT-GLOB: qsort begin");
        fflush(stdout);
        qsort(result->gl_pathv + existing, result->gl_pathc - existing,
              sizeof(char *), oct_glob_compare);
        puts("OCT-GLOB: qsort returned");
        fflush(stdout);
    }

    size_t slots = (result->gl_pathc + result->gl_offs + 1) * sizeof(char *);
    char **grown = realloc(result->gl_pathv, slots);
    if (grown == NULL) {
        return GLOB_NOSPACE;
    }
    result->gl_pathv = grown;
    if (result->gl_offs != 0) {
        memmove(result->gl_pathv + result->gl_offs, result->gl_pathv,
                result->gl_pathc * sizeof(char *));
        for (size_t index = 0; index < result->gl_offs; index++) {
            result->gl_pathv[index] = NULL;
        }
    }
    result->gl_pathv[result->gl_offs + result->gl_pathc] = NULL;
    return 0;
}

void globfree(glob_t *result)
{
    if (result == NULL) {
        return;
    }
    if (result->gl_pathv != NULL) {
        for (size_t index = 0; index < result->gl_pathc; index++) {
            free(result->gl_pathv[result->gl_offs + index]);
        }
        free(result->gl_pathv);
    }
    memset(result, 0, sizeof(*result));
}

/* ---- Line-oriented input ------------------------------------------------ */

ssize_t getdelim(char **line, size_t *capacity, int delimiter, FILE *stream)
{
    if (line == NULL || capacity == NULL || stream == NULL) {
        errno = EINVAL;
        return -1;
    }
    if (*line == NULL || *capacity == 0) {
        *capacity = 128;
        *line = malloc(*capacity);
        if (*line == NULL) {
            *capacity = 0;
            return -1;
        }
    }

    size_t length = 0;
    for (;;) {
        int character = getc(stream);
        if (character == EOF) {
            if (length == 0) {
                return -1;
            }
            break;
        }
        if (length + 1 >= *capacity) {
            size_t grown_capacity = *capacity * 2;
            char *grown = realloc(*line, grown_capacity);
            if (grown == NULL) {
                return -1;
            }
            *line = grown;
            *capacity = grown_capacity;
        }
        (*line)[length++] = (char)character;
        if (character == delimiter) {
            break;
        }
    }
    (*line)[length] = '\0';
    return (ssize_t)length;
}

ssize_t getline(char **line, size_t *capacity, FILE *stream)
{
    return getdelim(line, capacity, '\n', stream);
}

/* ---- Temporary directories ---------------------------------------------- */

char *mkdtemp(char *path_template)
{
    static const char alphabet[] =
        "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    if (path_template == NULL) {
        errno = EINVAL;
        return NULL;
    }
    size_t length = strlen(path_template);
    if (length < 6 || strcmp(path_template + length - 6, "XXXXXX") != 0) {
        errno = EINVAL;
        return NULL;
    }

    /*
     * The kernel has no mkdtemp syscall, so pick candidates from the clock and
     * a counter, and let mkdir decide: EEXIST means retry, anything else is
     * final. Uniqueness therefore rests on mkdir's atomicity, not on the
     * randomness of the suffix.
     */
    static unsigned int counter;
    for (int attempt = 0; attempt < 100; attempt++) {
        struct timespec now;
        uint64_t seed = (uint64_t)counter++ * 6364136223846793005ull;
        if (clock_gettime(CLOCK_MONOTONIC, &now) == 0) {
            seed ^= (uint64_t)now.tv_nsec * 2654435761ull;
        }
        for (size_t index = 0; index < 6; index++) {
            path_template[length - 6 + index] = alphabet[seed % (sizeof(alphabet) - 1)];
            seed /= (sizeof(alphabet) - 1);
        }
        if (mkdir(path_template, 0700) == 0) {
            return path_template;
        }
        if (errno != EEXIST) {
            return NULL;
        }
    }
    errno = EEXIST;
    return NULL;
}

/* ---- Process information ------------------------------------------------ */

int sched_getaffinity(pid_t process, size_t size, cpu_set_t *set)
{
    (void)process;
    if (set == NULL) {
        errno = EINVAL;
        return -1;
    }
    if (size < sizeof(cpu_set_t)) {
        errno = EINVAL;
        return -1;
    }
    /*
     * OCTAGRAM currently schedules on a single CPU and has no affinity
     * syscall, so report exactly one online processor rather than inventing a
     * wider mask.
     */
    memset(set, 0, sizeof(cpu_set_t));
    set->bits[0] |= 1u;
    return 0;
}

int sched_setaffinity(pid_t process, size_t size, const cpu_set_t *set)
{
    (void)process;
    if (set == NULL || size < sizeof(cpu_set_t)) {
        errno = EINVAL;
        return -1;
    }
    if ((set->bits[0] & 1u) == 0) {
        /* Masking out CPU 0 would leave nothing to run on. */
        errno = EINVAL;
        return -1;
    }
    return 0;
}

int sysinfo(struct sysinfo *information)
{
    if (information == NULL) {
        errno = EINVAL;
        return -1;
    }
    memset(information, 0, sizeof(*information));
    struct timespec uptime;
    if (clock_gettime(CLOCK_MONOTONIC, &uptime) != 0) {
        return -1;
    }
    information->uptime = uptime.tv_sec;
    information->procs = 1;
    information->mem_unit = 1;
    /*
     * There is no kernel call that reports total/free memory, and inventing
     * numbers would mislead callers (Rust's available_parallelism, LLVM's
     * memory limits). Report zero, which callers treat as "unknown".
     */
    return 0;
}

/* glibc's processor-count helpers over the _SC_NPROCESSORS_* queries the
 * kernel answers. A failure from sysconf becomes 1, matching what glibc does
 * when it cannot query the system, so callers never see a zero-sized pool. */
int get_nprocs(void)
{
    long online = sysconf(_SC_NPROCESSORS_ONLN);
    return online < 1 ? 1 : (int)online;
}

int get_nprocs_conf(void)
{
    long configured = sysconf(_SC_NPROCESSORS_CONF);
    return configured < 1 ? 1 : (int)configured;
}

unsigned long getauxval(unsigned long type)
{
    (void)type;
    /*
     * OCTAGRAM does not capture the initial stack's auxiliary vector, so there
     * is nothing truthful to return. ENOENT is the documented result for an
     * absent entry, which keeps glibc-compatible callers on their fallback
     * path instead of acting on fabricated values.
     */
    errno = ENOENT;
    return 0;
}

/* ---- getopt / getopt_long ----------------------------------------------- */

char *optarg;
int optind = 1;
int optopt;
int opterr = 1;
int optreset;

/* Index inside the current argv element; 1 means "start of a new element". */
static int oct_getopt_position = 1;

static const char *oct_program_name(void)
{
    if (program_invocation_short_name != NULL &&
        program_invocation_short_name[0] != '\0') {
        return program_invocation_short_name;
    }
    return "octagram";
}

static int oct_is_option(const char *argument)
{
    return argument[0] == '-' && argument[1] != '\0';
}

static int oct_next_option(char *const argv[], int start, int argc)
{
    for (int index = start; index < argc; index++) {
        if (oct_is_option(argv[index])) {
            return index;
        }
    }
    return -1;
}

static int oct_parse_long_option(int argc, char *const argv[],
                                 const struct option *long_options, int *long_index,
                                 int colon_mode, int name_offset)
{
    char *argument = argv[optind];
    char *name = argument + name_offset;
    char *equals = strchr(name, '=');
    size_t name_length = equals != NULL ? (size_t)(equals - name) : strlen(name);

    int match = -1;
    int ambiguous = 0;
    for (int index = 0; long_options[index].name != NULL; index++) {
        size_t candidate_length = strlen(long_options[index].name);
        if (candidate_length == name_length &&
            strncmp(name, long_options[index].name, name_length) == 0) {
            match = index;
            ambiguous = 0;
            break;
        }
        if (candidate_length > name_length &&
            strncmp(name, long_options[index].name, name_length) == 0) {
            if (match >= 0) {
                ambiguous = 1;
            } else {
                match = index;
            }
        }
    }

    if (match < 0 || ambiguous) {
        if (opterr && colon_mode == 0) {
            if (ambiguous) {
                fprintf(stderr, "%s: option '--%.*s' is ambiguous\n",
                        oct_program_name(), (int)name_length, name);
            } else {
                fprintf(stderr, "%s: unrecognized option '--%.*s'\n",
                        oct_program_name(), (int)name_length, name);
            }
        }
        optopt = 0;
        optind++;
        return '?';
    }

    const struct option *option = &long_options[match];
    if (long_index != NULL) {
        *long_index = match;
    }

    if (equals != NULL) {
        if (option->has_arg == no_argument) {
            if (opterr && colon_mode == 0) {
                fprintf(stderr, "%s: option '--%s' doesn't allow an argument\n",
                        oct_program_name(), option->name);
            }
            optind++;
            return '?';
        }
        optarg = equals + 1;
        optind++;
    } else if (option->has_arg == required_argument) {
        if (optind + 1 >= argc) {
            if (opterr && colon_mode == 0) {
                fprintf(stderr, "%s: option '--%s' requires an argument\n",
                        oct_program_name(), option->name);
            }
            optind++;
            return colon_mode != 0 ? ':' : '?';
        }
        optarg = argv[optind + 1];
        optind += 2;
    } else {
        optind++;
    }

    if (option->flag != NULL) {
        *option->flag = option->val;
        return 0;
    }
    return option->val;
}

static int oct_getopt_run(int argc, char *const argv[], const char *optstring,
                          const struct option *long_options, int *long_index,
                          int long_only)
{
    if (optreset) {
        optreset = 0;
        optind = 1;
        oct_getopt_position = 1;
    }
    if (optind == 0) {
        optind = 1;
    }
    if (argc <= 0 || argv == NULL) {
        return -1;
    }
    optarg = NULL;

    /* argv's elements are const-qualified in the POSIX signature, but GNU
     * getopt permutes them; the array itself is writable by contract. */
    char **mutable_argv = (char **)(uintptr_t)argv;

    const char *specification = optstring != NULL ? optstring : "";
    int stop_at_nonoption = 0;
    int return_nonoptions = 0;
    int colon_mode = 0;
    if (*specification == '+' || *specification == '-') {
        stop_at_nonoption = *specification == '+';
        return_nonoptions = *specification == '-';
        specification++;
    }
    if (*specification == ':') {
        colon_mode = 1;
        specification++;
    }

    for (;;) {
        if (oct_getopt_position == 1) {
            if (optind >= argc) {
                return -1;
            }
            if (!oct_is_option(argv[optind])) {
                if (stop_at_nonoption) {
                    return -1;
                }
                if (return_nonoptions) {
                    optarg = argv[optind++];
                    return 1;
                }
                /* GNU default: permute, so options after operands still parse. */
                int next = oct_next_option(argv, optind + 1, argc);
                if (next < 0) {
                    return -1;
                }
                char *moved = argv[next];
                memmove(&mutable_argv[optind + 1], &mutable_argv[optind],
                        (size_t)(next - optind) * sizeof(char *));
                mutable_argv[optind] = moved;
            }

            if (strcmp(argv[optind], "--") == 0) {
                optind++;
                return -1;
            }

            if (long_options != NULL) {
                if (argv[optind][1] == '-') {
                    return oct_parse_long_option(argc, argv, long_options,
                                                 long_index, colon_mode, 2);
                }
                if (long_only != 0) {
                    /* getopt_long_only: "-name" may be a long option. Only
                     * take that path when the text actually names an option;
                     * otherwise fall back to short-option parsing. */
                    char *probe = argv[optind] + 1;
                    char *equals = strchr(probe, '=');
                    size_t probe_length =
                        equals != NULL ? (size_t)(equals - probe) : strlen(probe);
                    int matches = 0;
                    for (int index = 0; long_options[index].name != NULL; index++) {
                        if (strlen(long_options[index].name) >= probe_length &&
                            strncmp(probe, long_options[index].name, probe_length) == 0) {
                            matches++;
                        }
                    }
                    if (matches == 1) {
                        return oct_parse_long_option(argc, argv, long_options,
                                                     long_index, colon_mode, 1);
                    }
                }
            }
        }

        const char *element = argv[optind];
        char option = element[oct_getopt_position];
        const char *found = NULL;
        for (const char *cursor = specification; *cursor != '\0'; cursor++) {
            if (*cursor == ':' ) {
                continue;
            }
            if (*cursor == option) {
                found = cursor;
                break;
            }
        }

        if (found == NULL) {
            optopt = (unsigned char)option;
            if (opterr && colon_mode == 0) {
                fprintf(stderr, "%s: invalid option -- '%c'\n", oct_program_name(),
                        option);
            }
            oct_getopt_position++;
            if (element[oct_getopt_position] == '\0') {
                oct_getopt_position = 1;
                optind++;
            }
            return '?';
        }

        int takes_argument = found[1] == ':';
        if (!takes_argument) {
            oct_getopt_position++;
            if (element[oct_getopt_position] == '\0') {
                oct_getopt_position = 1;
                optind++;
            }
            return (unsigned char)option;
        }

        if (element[oct_getopt_position + 1] != '\0') {
            optarg = (char *)element + oct_getopt_position + 1;
            oct_getopt_position = 1;
            optind++;
            return (unsigned char)option;
        }
        if (optind + 1 < argc) {
            optarg = argv[optind + 1];
            oct_getopt_position = 1;
            optind += 2;
            return (unsigned char)option;
        }

        optopt = (unsigned char)option;
        oct_getopt_position = 1;
        optind++;
        if (opterr && colon_mode == 0) {
            fprintf(stderr, "%s: option requires an argument -- '%c'\n",
                    oct_program_name(), option);
        }
        return colon_mode != 0 ? ':' : '?';
    }
}

int getopt(int argc, char *const argv[], const char *optstring)
{
    return oct_getopt_run(argc, argv, optstring, NULL, NULL, 0);
}

int getopt_long(int argc, char *const argv[], const char *optstring,
                const struct option *long_options, int *long_index)
{
    return oct_getopt_run(argc, argv, optstring, long_options, long_index, 0);
}

int getopt_long_only(int argc, char *const argv[], const char *optstring,
                     const struct option *long_options, int *long_index)
{
    return oct_getopt_run(argc, argv, optstring, long_options, long_index, 1);
}

/* ---- GNU error() and BSD err.h ------------------------------------------ */

unsigned int error_message_count;
int error_one_per_line;
char *program_invocation_name = (char *)"";
char *program_invocation_short_name = (char *)"";

static void oct_diagnostic(int error_number, const char *file_name,
                           unsigned int line_number, const char *format,
                           va_list arguments)
{
    fprintf(stderr, "%s: ", oct_program_name());
    if (format != NULL) {
        vfprintf(stderr, format, arguments);
    }
    if (error_number != 0) {
        fprintf(stderr, ": %s", strerror(error_number));
    }
    if (file_name != NULL) {
        fprintf(stderr, ": %s:%u", file_name, line_number);
    }
    fputc('\n', stderr);
    fflush(stderr);
}

void error(int status, int error_number, const char *format, ...)
{
    va_list arguments;
    va_start(arguments, format);
    oct_diagnostic(error_number, NULL, 0, format, arguments);
    va_end(arguments);
    error_message_count++;
    if (status != 0) {
        exit(status);
    }
}

void error_at_line(int status, int error_number, const char *file_name,
                   unsigned int line_number, const char *format, ...)
{
    static const char *last_file;
    static unsigned int last_line;
    if (error_one_per_line && last_file != NULL && last_line == line_number &&
        strcmp(last_file, file_name) == 0) {
        return;
    }
    last_file = file_name;
    last_line = line_number;

    va_list arguments;
    va_start(arguments, format);
    oct_diagnostic(error_number, file_name, line_number, format, arguments);
    va_end(arguments);
    error_message_count++;
    if (status != 0) {
        exit(status);
    }
}

void vwarnx(const char *format, va_list arguments)
{
    oct_diagnostic(0, NULL, 0, format, arguments);
}

void vwarn(const char *format, va_list arguments)
{
    oct_diagnostic(errno, NULL, 0, format, arguments);
}

void warnx(const char *format, ...)
{
    va_list arguments;
    va_start(arguments, format);
    oct_diagnostic(0, NULL, 0, format, arguments);
    va_end(arguments);
}

void warn(const char *format, ...)
{
    va_list arguments;
    int saved = errno;
    va_start(arguments, format);
    oct_diagnostic(saved, NULL, 0, format, arguments);
    va_end(arguments);
}

void verrx(int status, const char *format, va_list arguments)
{
    oct_diagnostic(0, NULL, 0, format, arguments);
    exit(status);
}

void errx(int status, const char *format, ...)
{
    va_list arguments;
    va_start(arguments, format);
    oct_diagnostic(0, NULL, 0, format, arguments);
    va_end(arguments);
    exit(status);
}

void verr(int status, const char *format, va_list arguments)
{
    int saved = errno;
    oct_diagnostic(saved, NULL, 0, format, arguments);
    exit(status);
}

void err(int status, const char *format, ...)
{
    va_list arguments;
    int saved = errno;
    va_start(arguments, format);
    oct_diagnostic(saved, NULL, 0, format, arguments);
    va_end(arguments);
    exit(status);
}

/* ---- Version-aware comparison ------------------------------------------ */

int strverscmp(const char *left, const char *right)
{
    /*
     * Compare runs of digits as numbers so "file10" sorts after "file9", and
     * order equal numeric runs by their leading zeros ("01" before "1"), which
     * is the behaviour strverscmp is used for. Non-digit bytes compare by
     * unsigned value.
     */
    const unsigned char *l = (const unsigned char *)left;
    const unsigned char *r = (const unsigned char *)right;

    while (*l != '\0' && *r != '\0') {
        if (*l >= '0' && *l <= '9' && *r >= '0' && *r <= '9') {
            const unsigned char *l_end = l;
            const unsigned char *r_end = r;
            while (*l_end >= '0' && *l_end <= '9') {
                l_end++;
            }
            while (*r_end >= '0' && *r_end <= '9') {
                r_end++;
            }
            const unsigned char *l_digits = l;
            const unsigned char *r_digits = r;
            while (l_digits < l_end && *l_digits == '0') {
                l_digits++;
            }
            while (r_digits < r_end && *r_digits == '0') {
                r_digits++;
            }
            size_t l_length = (size_t)(l_end - l_digits);
            size_t r_length = (size_t)(r_end - r_digits);
            if (l_length != r_length) {
                return l_length < r_length ? -1 : 1;
            }
            int ordering = memcmp(l_digits, r_digits, l_length);
            if (ordering != 0) {
                return ordering < 0 ? -1 : 1;
            }
            size_t l_zeros = (size_t)(l_digits - l);
            size_t r_zeros = (size_t)(r_digits - r);
            if (l_zeros != r_zeros) {
                return l_zeros < r_zeros ? -1 : 1;
            }
            l = l_end;
            r = r_end;
            continue;
        }
        if (*l != *r) {
            return *l < *r ? -1 : 1;
        }
        l++;
        r++;
    }
    if (*l == *r) {
        return 0;
    }
    return *l == '\0' ? -1 : 1;
}

/* ---- Memory advice ------------------------------------------------------ */

int madvise(void *address, size_t length, int advice)
{
    (void)address;
    (void)length;
    (void)advice;
    /*
     * The kernel has no madvise equivalent: mappings are anonymous and the
     * page tables are managed by the kernel's own allocator. Report ENOSYS so
     * callers keep their fallback behaviour instead of assuming a hint was
     * applied.
     */
    errno = ENOSYS;
    return -1;
}

void *mremap(void *old_address, size_t old_size, size_t new_size, int flags, ...)
{
    (void)old_address;
    (void)old_size;
    (void)new_size;
    (void)flags;
    errno = ENOSYS;
    return MAP_FAILED;
}

/* ---- String search helpers ---------------------------------------------- */

/* GNU strchrnul: like strchr but returns the terminator when the character is
 * absent, which lets callers avoid a second strlen. */
char *strchrnul(const char *text, int character)
{
    const unsigned char wanted = (unsigned char)character;
    const unsigned char *cursor = (const unsigned char *)text;
    while (*cursor != '\0' && *cursor != wanted) {
        ++cursor;
    }
    return (char *)cursor;
}

char *strcasestr(const char *haystack, const char *needle)
{
    if (haystack == NULL || needle == NULL) {
        return NULL;
    }
    if (needle[0] == '\0') {
        return (char *)haystack;
    }
    const size_t length = strlen(needle);
    for (const char *cursor = haystack; cursor[0] != '\0'; ++cursor) {
        size_t matched = 0;
        while (matched < length && cursor[matched] != '\0' &&
               tolower((unsigned char)cursor[matched]) ==
                   tolower((unsigned char)needle[matched])) {
            ++matched;
        }
        if (matched == length) {
            return (char *)cursor;
        }
    }
    return NULL;
}

/* ---- getsubopt ---------------------------------------------------------- */

/* Splits `name=value,name=value` option arguments against a token list. The
 * returned index is the matching token, or -1 when the option is unknown; the
 * value (if any) is NUL terminated in place and *option_pointer advances. */
int getsubopt(char **option_pointer, char *const *tokens, char **value_pointer)
{
    if (option_pointer == NULL || tokens == NULL || value_pointer == NULL) {
        errno = EINVAL;
        return -1;
    }
    char *option = *option_pointer;
    *value_pointer = NULL;
    if (option == NULL || option[0] == '\0') {
        *option_pointer = NULL;
        return -1;
    }

    char *separator = strchr(option, ',');
    const size_t option_length =
        separator != NULL ? (size_t)(separator - option) : strlen(option);
    char *equals = memchr(option, '=', option_length);
    const size_t name_length =
        equals != NULL ? (size_t)(equals - option) : option_length;

    int index = 0;
    for (; tokens[index] != NULL; ++index) {
        if (strlen(tokens[index]) == name_length &&
            memcmp(tokens[index], option, name_length) == 0) {
            break;
        }
    }
    if (equals != NULL) {
        if (separator != NULL) {
            separator[0] = '\0';
        }
        *value_pointer = equals + 1;
    }
    *option_pointer = separator != NULL ? separator + 1 : NULL;
    return tokens[index] != NULL ? index : -1;
}

/* ---- scandir ------------------------------------------------------------ */

static size_t dirent_storage_size(const struct dirent *entry)
{
    return offsetof(struct dirent, d_name) + strlen(entry->d_name) + 1;
}

static struct dirent *copy_dirent(const struct dirent *entry)
{
    const size_t size = dirent_storage_size(entry);
    struct dirent *copy = malloc(size);
    if (copy == NULL) {
        return NULL;
    }
    memcpy(copy, entry, size);
    copy->d_reclen = (uint16_t)size;
    return copy;
}

/* The caller's comparison takes slots, but an insertion sort here keeps the
 * sort reentrant (a qsort trampoline would need a global function pointer). */
static int compare_dirent_slots(const struct dirent *left,
                                const struct dirent *right,
                                int (*compar)(const struct dirent **,
                                              const struct dirent **))
{
    return compar(&left, &right);
}

static void sort_dirent_slots(struct dirent **entries, size_t count,
                              int (*compar)(const struct dirent **,
                                            const struct dirent **))
{
    for (size_t index = 1; index < count; ++index) {
        struct dirent *current = entries[index];
        size_t position = index;
        while (position > 0 &&
               compare_dirent_slots(entries[position - 1], current, compar) > 0) {
            entries[position] = entries[position - 1];
            --position;
        }
        entries[position] = current;
    }
}

static int collect_directory(DIR *directory, struct dirent ***namelist,
                             int (*filter)(const struct dirent *),
                             int (*compar)(const struct dirent **,
                                           const struct dirent **))
{
    size_t count = 0;
    size_t capacity = 16;
    struct dirent **entries = malloc(capacity * sizeof(*entries));
    if (entries == NULL) {
        errno = ENOMEM;
        return -1;
    }

    for (;;) {
        errno = 0;
        struct dirent *entry = readdir(directory);
        if (entry == NULL) {
            if (errno != 0) {
                const int error = errno;
                for (size_t index = 0; index < count; ++index) {
                    free(entries[index]);
                }
                free(entries);
                errno = error;
                return -1;
            }
            break;
        }
        if (filter != NULL && filter(entry) == 0) {
            continue;
        }
        if (count == capacity) {
            const size_t grown_capacity = capacity * 2;
            struct dirent **grown =
                realloc(entries, grown_capacity * sizeof(*entries));
            if (grown == NULL) {
                for (size_t index = 0; index < count; ++index) {
                    free(entries[index]);
                }
                free(entries);
                errno = ENOMEM;
                return -1;
            }
            entries = grown;
            capacity = grown_capacity;
        }
        struct dirent *copy = copy_dirent(entry);
        if (copy == NULL) {
            for (size_t index = 0; index < count; ++index) {
                free(entries[index]);
            }
            free(entries);
            errno = ENOMEM;
            return -1;
        }
        entries[count++] = copy;
    }

    if (compar != NULL) {
        sort_dirent_slots(entries, count, compar);
    }
    entries[count] = NULL;
    *namelist = entries;
    return (int)count;
}

int scandir(const char *path, struct dirent ***namelist,
            int (*filter)(const struct dirent *),
            int (*compar)(const struct dirent **, const struct dirent **))
{
    if (path == NULL || namelist == NULL) {
        errno = EINVAL;
        return -1;
    }
    DIR *directory = opendir(path);
    if (directory == NULL) {
        return -1;
    }
    const int count = collect_directory(directory, namelist, filter, compar);
    const int saved = errno;
    closedir(directory);
    errno = saved;
    return count;
}

int scandirat(int dirfd, const char *path, struct dirent ***namelist,
              int (*filter)(const struct dirent *),
              int (*compar)(const struct dirent **, const struct dirent **))
{
    if (path == NULL || namelist == NULL) {
        errno = EINVAL;
        return -1;
    }
    if (dirfd == AT_FDCWD || path[0] == '/') {
        return scandir(path, namelist, filter, compar);
    }
    const int descriptor = openat(dirfd, path, O_RDONLY | O_DIRECTORY, 0);
    if (descriptor < 0) {
        return -1;
    }
    DIR *directory = fdopendir(descriptor);
    if (directory == NULL) {
        const int error = errno;
        close(descriptor);
        errno = error;
        return -1;
    }
    const int count = collect_directory(directory, namelist, filter, compar);
    const int saved = errno;
    closedir(directory);
    errno = saved;
    return count;
}

int alphasort(const struct dirent **left, const struct dirent **right)
{
    if (left == NULL || right == NULL || *left == NULL || *right == NULL) {
        return 0;
    }
    return strcoll((*left)->d_name, (*right)->d_name);
}

int versionsort(const struct dirent **left, const struct dirent **right)
{
    if (left == NULL || right == NULL || *left == NULL || *right == NULL) {
        return 0;
    }
    return strverscmp((*left)->d_name, (*right)->d_name);
}

/* ---- tsearch: balanced binary search tree -------------------------------- */

/* POSIX only promises a binary search tree; the nodes here are kept AVL
 * balanced so lookups stay logarithmic when keys arrive in sorted order
 * (which is exactly how a compiler inserts source locations). */
struct search_node {
    const void *key;
    struct search_node *left;
    struct search_node *right;
    int height;
};

static int search_height(const struct search_node *node)
{
    return node == NULL ? 0 : node->height;
}

static void search_update_height(struct search_node *node)
{
    const int left = search_height(node->left);
    const int right = search_height(node->right);
    node->height = (left > right ? left : right) + 1;
}

static struct search_node *search_rotate_left(struct search_node *node)
{
    struct search_node *pivot = node->right;
    node->right = pivot->left;
    pivot->left = node;
    search_update_height(node);
    search_update_height(pivot);
    return pivot;
}

static struct search_node *search_rotate_right(struct search_node *node)
{
    struct search_node *pivot = node->left;
    node->left = pivot->right;
    pivot->right = node;
    search_update_height(node);
    search_update_height(pivot);
    return pivot;
}

static struct search_node *search_rebalance(struct search_node *node)
{
    search_update_height(node);
    const int balance = search_height(node->left) - search_height(node->right);
    if (balance > 1) {
        if (search_height(node->left->left) < search_height(node->left->right)) {
            node->left = search_rotate_left(node->left);
        }
        return search_rotate_right(node);
    }
    if (balance < -1) {
        if (search_height(node->right->right) < search_height(node->right->left)) {
            node->right = search_rotate_right(node->right);
        }
        return search_rotate_left(node);
    }
    return node;
}

void *tsearch(const void *key, void **root_pointer,
              int (*compar)(const void *, const void *))
{
    if (root_pointer == NULL || compar == NULL) {
        errno = EINVAL;
        return NULL;
    }
    struct search_node *node = *root_pointer;
    if (node == NULL) {
        struct search_node *created = malloc(sizeof(*created));
        if (created == NULL) {
            errno = ENOMEM;
            return NULL;
        }
        created->key = key;
        created->left = NULL;
        created->right = NULL;
        created->height = 1;
        *root_pointer = created;
        return created;
    }
    const int direction = compar(key, node->key);
    if (direction == 0) {
        return node;
    }
    void *found;
    if (direction < 0) {
        found = tsearch(key, (void **)&node->left, compar);
    } else {
        found = tsearch(key, (void **)&node->right, compar);
    }
    if (found == NULL) {
        return NULL;
    }
    *root_pointer = search_rebalance(node);
    return found;
}

void *tfind(const void *key, void *const *root_pointer,
            int (*compar)(const void *, const void *))
{
    if (root_pointer == NULL || compar == NULL) {
        errno = EINVAL;
        return NULL;
    }
    struct search_node *node = *root_pointer;
    while (node != NULL) {
        const int direction = compar(key, node->key);
        if (direction == 0) {
            return node;
        }
        node = direction < 0 ? node->left : node->right;
    }
    return NULL;
}

static struct search_node *search_remove(struct search_node *node,
                                         const void *key,
                                         int (*compar)(const void *, const void *),
                                         struct search_node **removed)
{
    if (node == NULL) {
        return NULL;
    }
    const int direction = compar(key, node->key);
    if (direction < 0) {
        node->left = search_remove(node->left, key, compar, removed);
    } else if (direction > 0) {
        node->right = search_remove(node->right, key, compar, removed);
    } else if (node->left == NULL || node->right == NULL) {
        struct search_node *child = node->left != NULL ? node->left : node->right;
        *removed = node;
        return child;
    } else {
        /* Two children: pull the successor key up and delete that node, which
         * has no left child, so the rotation path stays simple. */
        struct search_node *successor = node->right;
        while (successor->left != NULL) {
            successor = successor->left;
        }
        node->key = successor->key;
        node->right = search_remove(node->right, successor->key, compar, removed);
    }
    return search_rebalance(node);
}

void *tdelete(const void *key, void **root_pointer,
              int (*compar)(const void *, const void *))
{
    if (root_pointer == NULL || compar == NULL) {
        errno = EINVAL;
        return NULL;
    }
    struct search_node *node = *root_pointer;
    struct search_node *parent = NULL;
    while (node != NULL) {
        const int direction = compar(key, node->key);
        if (direction == 0) {
            break;
        }
        parent = node;
        node = direction < 0 ? node->left : node->right;
    }
    if (node == NULL) {
        return NULL;
    }
    struct search_node *removed = NULL;
    *root_pointer = search_remove(*root_pointer, key, compar, &removed);
    if (removed == NULL) {
        return NULL;
    }
    free(removed);
    return parent;
}

static void search_walk(const struct search_node *node,
                        void (*action)(const void *, VISIT, int), int depth)
{
    if (node == NULL) {
        return;
    }
    if (node->left == NULL && node->right == NULL) {
        action(node, leaf, depth);
        return;
    }
    action(node, preorder, depth);
    search_walk(node->left, action, depth + 1);
    action(node, postorder, depth);
    search_walk(node->right, action, depth + 1);
    action(node, endorder, depth);
}

void twalk(const void *root, void (*action)(const void *, VISIT, int))
{
    if (root == NULL || action == NULL) {
        return;
    }
    search_walk(root, action, 0);
}

void tdestroy(void *root, void (*free_function)(void *))
{
    struct search_node *node = root;
    if (node == NULL) {
        return;
    }
    tdestroy(node->left, free_function);
    tdestroy(node->right, free_function);
    if (free_function != NULL) {
        free_function((void *)node->key);
    }
    free(node);
}

/* ---- insque/remque ------------------------------------------------------ */

/* The POSIX intrusive list stores its links in the first two words of the
 * element, so a shadow struct is enough to walk it without knowing the rest. */
struct octagram_qelem {
    struct octagram_qelem *q_forw;
    struct octagram_qelem *q_back;
};

void insque(void *element, void *predecessor)
{
    if (element == NULL) {
        return;
    }
    struct octagram_qelem *node = element;
    struct octagram_qelem *pred = predecessor;
    if (pred == NULL) {
        node->q_forw = NULL;
        node->q_back = NULL;
        return;
    }
    node->q_forw = pred->q_forw;
    node->q_back = pred;
    if (pred->q_forw != NULL) {
        pred->q_forw->q_back = node;
    }
    pred->q_forw = node;
}

void remque(void *element)
{
    if (element == NULL) {
        return;
    }
    struct octagram_qelem *node = element;
    if (node->q_forw != NULL) {
        node->q_forw->q_back = node->q_back;
    }
    if (node->q_back != NULL) {
        node->q_back->q_forw = node->q_forw;
    }
}

/* ---- basename/dirname --------------------------------------------------- */

/* Two functions share the basename name. The one declared by <string.h> is the
 * GNU form: it never modifies its argument and simply returns the part after
 * the last slash. <libgen.h> renames basename to __xpg_basename, which is the
 * POSIX form that may rewrite the buffer. */
char *basename(const char *path)
{
    if (path == NULL) {
        return NULL;
    }
    char *slash = strrchr(path, '/');
    return slash != NULL ? slash + 1 : (char *)path;
}

char *__xpg_basename(char *path)
{
    if (path == NULL || path[0] == '\0') {
        static char dot[] = ".";
        return dot;
    }
    size_t length = strlen(path);
    while (length > 1 && path[length - 1] == '/') {
        path[--length] = '\0';
    }
    if (length == 1 && path[0] == '/') {
        return path;
    }
    char *slash = strrchr(path, '/');
    if (slash == NULL) {
        return path;
    }
    return slash + 1;
}

char *dirname(char *path)
{
    static char dot[] = ".";
    static char root[] = "/";
    if (path == NULL || path[0] == '\0') {
        return dot;
    }
    size_t length = strlen(path);
    while (length > 1 && path[length - 1] == '/') {
        path[--length] = '\0';
    }
    char *slash = strrchr(path, '/');
    if (slash == NULL) {
        return dot;
    }
    if (slash == path) {
        return root;
    }
    while (slash > path && slash[-1] == '/') {
        --slash;
    }
    slash[0] = '\0';
    return path;
}

/* ---- wordexp ------------------------------------------------------------ */

/* Field builder for the final we_wordv array. Every string is owned. */
struct wordexp_fields {
    char **items;
    size_t count;
    size_t capacity;
};

/* One field being assembled. `quoted` remembers that the field was quoted, so
 * an empty result from "" is kept while an empty unquoted expansion is dropped,
 * and `glob` records an unquoted wildcard for later pathname expansion. */
struct wordexp_word {
    char *text;
    size_t length;
    size_t capacity;
    int quoted;
    int glob;
};

static int wordexp_fields_push(struct wordexp_fields *fields, char *item)
{
    if (fields->count == fields->capacity) {
        const size_t capacity = fields->capacity != 0 ? fields->capacity * 2 : 8;
        char **grown = realloc(fields->items, capacity * sizeof(*grown));
        if (grown == NULL) {
            return -1;
        }
        fields->items = grown;
        fields->capacity = capacity;
    }
    fields->items[fields->count++] = item;
    return 0;
}

static void wordexp_fields_release(struct wordexp_fields *fields)
{
    for (size_t index = 0; index < fields->count; ++index) {
        free(fields->items[index]);
    }
    free(fields->items);
    fields->items = NULL;
    fields->count = 0;
    fields->capacity = 0;
}

static int wordexp_word_append(struct wordexp_word *word, const char *text,
                               size_t count)
{
    if (word->length + count + 1 > word->capacity) {
        size_t capacity = word->capacity != 0 ? word->capacity : 32;
        while (capacity < word->length + count + 1) {
            capacity *= 2;
        }
        char *grown = realloc(word->text, capacity);
        if (grown == NULL) {
            return -1;
        }
        word->text = grown;
        word->capacity = capacity;
    }
    if (count != 0) {
        memcpy(word->text + word->length, text, count);
        word->length += count;
    }
    word->text[word->length] = '\0';
    return 0;
}

static int wordexp_word_append_char(struct wordexp_word *word, char character)
{
    return wordexp_word_append(word, &character, 1);
}

static void wordexp_word_release(struct wordexp_word *word)
{
    free(word->text);
    word->text = NULL;
    word->length = 0;
    word->capacity = 0;
    word->quoted = 0;
    word->glob = 0;
}

/* Expands $NAME or ${NAME} at *cursor. A lone '$' stays literal. */
static int wordexp_expand_variable(const char **cursor, struct wordexp_word *word,
                                   int flags, int *error)
{
    const char *text = *cursor;
    const char *name_start;
    size_t name_length;
    const char *after;

    if (text[1] == '{') {
        name_start = text + 2;
        const char *closing = strchr(name_start, '}');
        if (closing == NULL) {
            *error = WRDE_SYNTAX;
            return -1;
        }
        name_length = (size_t)(closing - name_start);
        after = closing + 1;
    } else {
        name_start = text + 1;
        name_length = 0;
        while (isalnum((unsigned char)name_start[name_length]) ||
               name_start[name_length] == '_') {
            ++name_length;
        }
        after = name_start + name_length;
    }

    if (name_length == 0) {
        *cursor = text + 1;
        if (wordexp_word_append_char(word, '$') < 0) {
            *error = WRDE_NOSPACE;
            return -1;
        }
        return 0;
    }

    char name[256];
    if (name_length >= sizeof(name)) {
        *error = WRDE_BADVAL;
        return -1;
    }
    memcpy(name, name_start, name_length);
    name[name_length] = '\0';

    const char *value = getenv(name);
    if (value == NULL) {
        if ((flags & WRDE_UNDEF) != 0) {
            *error = WRDE_BADVAL;
            return -1;
        }
        value = "";
    }
    if (wordexp_word_append(word, value, strlen(value)) < 0) {
        *error = WRDE_NOSPACE;
        return -1;
    }
    *cursor = after;
    return 0;
}

/* Publishes the finished field: pathname expansion for unquoted wildcards, a
 * plain copy otherwise. Unquoted empty fields disappear. */
static int wordexp_word_finish(struct wordexp_fields *fields,
                               struct wordexp_word *word, int *error)
{
    if (word->length == 0 && word->quoted == 0) {
        wordexp_word_release(word);
        return 0;
    }
    if (word->text == NULL) {
        /*
         * An explicitly quoted empty word (for example, "" or '') is still
         * one field.  It has no appended bytes, so the builder has not
         * allocated storage yet; materialize the terminating NUL before
         * transferring ownership to the result array.
         */
        if (word->length == 0 && word->quoted != 0) {
            word->text = malloc(1);
            if (word->text == NULL) {
                wordexp_word_release(word);
                *error = WRDE_NOSPACE;
                return -1;
            }
            word->text[0] = '\0';
            word->capacity = 1;
        } else {
            wordexp_word_release(word);
            return 0;
        }
    }

    if (word->glob != 0) {
        glob_t matches;
        memset(&matches, 0, sizeof(matches));
        puts("OCT-WORDEXP: glob begin");
        fflush(stdout);
        int glob_status = glob(word->text, GLOB_NOCHECK, NULL, &matches);
        puts("OCT-WORDEXP: glob returned");
        fflush(stdout);
        if (glob_status != 0) {
            wordexp_word_release(word);
            *error = WRDE_NOSPACE;
            return -1;
        }
        for (size_t index = 0; index < matches.gl_pathc; ++index) {
            /*
             * glob() reserves gl_offs leading NULL entries in gl_pathv.
             * Iterate over the actual pathname slots, not the reserved
             * offset area (which is NULL for this call).
             */
            char *copy = strdup(matches.gl_pathv[matches.gl_offs + index]);
            if (copy == NULL || wordexp_fields_push(fields, copy) < 0) {
                free(copy);
                globfree(&matches);
                wordexp_word_release(word);
                *error = WRDE_NOSPACE;
                return -1;
            }
        }
        globfree(&matches);
        wordexp_word_release(word);
        return 0;
    }

    if (wordexp_fields_push(fields, word->text) < 0) {
        wordexp_word_release(word);
        *error = WRDE_NOSPACE;
        return -1;
    }
    /* Ownership moved into the field array. */
    word->text = NULL;
    wordexp_word_release(word);
    return 0;
}

static int wordexp_parse(const char *words, struct wordexp_fields *fields,
                         int flags, int *error)
{
    const char *cursor = words;
    struct wordexp_word word;
    memset(&word, 0, sizeof(word));
    int in_word = 0;

    while (*cursor != '\0') {
        const char character = *cursor;

        if (in_word == 0 && isspace((unsigned char)character)) {
            ++cursor;
            continue;
        }

        switch (character) {
        case ' ':
        case '\t':
        case '\n':
            if (wordexp_word_finish(fields, &word, error) < 0) {
                return -1;
            }
            in_word = 0;
            ++cursor;
            continue;
        case '\'':
            word.quoted = 1;
            in_word = 1;
            ++cursor;
            while (*cursor != '\0' && *cursor != '\'') {
                if (wordexp_word_append_char(&word, *cursor++) < 0) {
                    *error = WRDE_NOSPACE;
                    return -1;
                }
            }
            if (*cursor == '\0') {
                *error = WRDE_SYNTAX;
                return -1;
            }
            ++cursor;
            continue;
        case '"':
            word.quoted = 1;
            in_word = 1;
            ++cursor;
            while (*cursor != '\0' && *cursor != '"') {
                if (*cursor == '\\' && cursor[1] != '\0') {
                    const char escaped = cursor[1];
                    if (escaped == '$' || escaped == '"' || escaped == '\\') {
                        if (wordexp_word_append_char(&word, escaped) < 0) {
                            *error = WRDE_NOSPACE;
                            return -1;
                        }
                        cursor += 2;
                        continue;
                    }
                }
                if (*cursor == '$') {
                    if (wordexp_expand_variable(&cursor, &word, flags, error) < 0) {
                        return -1;
                    }
                    continue;
                }
                if (*cursor == '`' || (*cursor == '$' && cursor[1] == '(')) {
                    *error = WRDE_CMDSUB;
                    return -1;
                }
                if (wordexp_word_append_char(&word, *cursor++) < 0) {
                    *error = WRDE_NOSPACE;
                    return -1;
                }
            }
            if (*cursor == '\0') {
                *error = WRDE_SYNTAX;
                return -1;
            }
            ++cursor;
            continue;
        case '\\':
            if (cursor[1] == '\0') {
                *error = WRDE_SYNTAX;
                return -1;
            }
            word.quoted = 1;
            in_word = 1;
            if (wordexp_word_append_char(&word, cursor[1]) < 0) {
                *error = WRDE_NOSPACE;
                return -1;
            }
            cursor += 2;
            continue;
        case '$':
            in_word = 1;
            if (cursor[1] == '(') {
                *error = WRDE_CMDSUB;
                return -1;
            }
            if (wordexp_expand_variable(&cursor, &word, flags, error) < 0) {
                return -1;
            }
            continue;
        case '`':
            *error = WRDE_CMDSUB;
            return -1;
        case '~':
            in_word = 1;
            if (word.length == 0 && (cursor[1] == '\0' || cursor[1] == '/')) {
                const char *home = getenv("HOME");
                if (home == NULL) {
                    if ((flags & WRDE_UNDEF) != 0) {
                        *error = WRDE_BADVAL;
                        return -1;
                    }
                    home = "";
                }
                if (wordexp_word_append(&word, home, strlen(home)) < 0) {
                    *error = WRDE_NOSPACE;
                    return -1;
                }
                ++cursor;
                continue;
            }
            if (wordexp_word_append_char(&word, character) < 0) {
                *error = WRDE_NOSPACE;
                return -1;
            }
            ++cursor;
            continue;
        case '|':
        case '&':
        case ';':
        case '<':
        case '>':
        case '(':
        case ')':
        case '{':
        case '}':
            *error = WRDE_BADCHAR;
            return -1;
        case '*':
        case '?':
        case '[':
            word.glob = 1;
            in_word = 1;
            if (wordexp_word_append_char(&word, character) < 0) {
                *error = WRDE_NOSPACE;
                return -1;
            }
            ++cursor;
            continue;
        default:
            in_word = 1;
            if (wordexp_word_append_char(&word, character) < 0) {
                *error = WRDE_NOSPACE;
                return -1;
            }
            ++cursor;
            continue;
        }
    }

    if (wordexp_word_finish(fields, &word, error) < 0) {
        return -1;
    }
    return 0;
}

int wordexp(const char *words, wordexp_t *result, int flags)
{
    if (words == NULL || result == NULL) {
        return WRDE_SYNTAX;
    }
    if ((flags & WRDE_REUSE) != 0) {
        wordfree(result);
    }

    struct wordexp_fields fields;
    memset(&fields, 0, sizeof(fields));
    int error = 0;
    if (wordexp_parse(words, &fields, flags, &error) < 0) {
        wordexp_fields_release(&fields);
        return error != 0 ? error : WRDE_SYNTAX;
    }

    const size_t offset = (flags & WRDE_DOOFFS) != 0 ? result->we_offs : 0;
    size_t existing = 0;
    size_t existing_offset = 0;
    char **previous = NULL;
    if ((flags & WRDE_APPEND) != 0 && result->we_wordv != NULL) {
        previous = result->we_wordv;
        existing = result->we_wordc;
        existing_offset = offset;
    }

    const size_t total = existing + fields.count;
    char **array = malloc((total + offset + 1) * sizeof(*array));
    if (array == NULL) {
        wordexp_fields_release(&fields);
        return WRDE_NOSPACE;
    }

    size_t slot = 0;
    for (; slot < offset; ++slot) {
        array[slot] = NULL;
    }
    if (previous != NULL) {
        /* APPEND keeps the earlier words in front of the new ones. */
        for (size_t index = 0; index < existing; ++index) {
            array[existing_offset + index] = previous[existing_offset + index];
        }
        free(previous);
        slot = existing_offset + existing;
    } else {
        slot = offset;
    }
    for (size_t index = 0; index < fields.count; ++index) {
        array[slot++] = fields.items[index];
    }
    array[slot] = NULL;

    result->we_wordv = array;
    result->we_wordc = total;
    result->we_offs = offset;
    free(fields.items);
    return 0;
}

void wordfree(wordexp_t *result)
{
    if (result == NULL || result->we_wordv == NULL) {
        return;
    }
    const size_t offset = result->we_offs;
    for (size_t index = 0; index < result->we_wordc; ++index) {
        free(result->we_wordv[offset + index]);
    }
    free(result->we_wordv);
    result->we_wordv = NULL;
    result->we_wordc = 0;
}

/* ---- nftw --------------------------------------------------------------- */

static int nftw_walk(const char *path, size_t path_length,
                     int (*function)(const char *, const struct stat *, int,
                                     struct FTW *),
                     int flags, int depth, dev_t device)
{
    struct stat metadata;
    memset(&metadata, 0, sizeof(metadata));
    int status = (flags & FTW_PHYS) != 0 ? lstat(path, &metadata)
                                       : stat(path, &metadata);
    struct FTW info;
    info.level = depth;
    info.base = (int)(path_length - strlen(basename(path)));

    if (status < 0) {
        return function(path, &metadata, FTW_NS, &info);
    }

    if (S_ISLNK(metadata.st_mode)) {
        if ((flags & FTW_PHYS) != 0) {
            return function(path, &metadata, FTW_SL, &info);
        }
        /* A dangling symlink reports FTW_SLN instead of vanishing. */
        struct stat target;
        if (stat(path, &target) < 0) {
            return function(path, &metadata, FTW_SLN, &info);
        }
        return function(path, &target, FTW_F, &info);
    }

    if (!S_ISDIR(metadata.st_mode)) {
        return function(path, &metadata, FTW_F, &info);
    }

    if ((flags & FTW_DEPTH) == 0) {
        const int result = function(path, &metadata, FTW_D, &info);
        if (result != 0) {
            return result;
        }
    }

    if ((flags & FTW_MOUNT) != 0 && metadata.st_dev != device) {
        return 0;
    }

    DIR *directory = opendir(path);
    if (directory == NULL) {
        const int result = function(path, &metadata, FTW_DNR, &info);
        return result != 0 ? result : 0;
    }

    int walk_status = 0;
    for (;;) {
        errno = 0;
        struct dirent *entry = readdir(directory);
        if (entry == NULL) {
            break;
        }
        const char *name = entry->d_name;
        if (strcmp(name, ".") == 0 || strcmp(name, "..") == 0) {
            continue;
        }
        const size_t name_length = strlen(name);
        const size_t child_length = path_length + 1 + name_length;
        char *child = malloc(child_length + 1);
        if (child == NULL) {
            walk_status = -1;
            break;
        }
        memcpy(child, path, path_length);
        size_t position = path_length;
        if (position == 0 || child[position - 1] != '/') {
            child[position++] = '/';
        }
        memcpy(child + position, name, name_length + 1);
        walk_status = nftw_walk(child, position + name_length, function, flags,
                               depth + 1, device);
        free(child);
        if (walk_status != 0) {
            break;
        }
    }
    closedir(directory);

    if (walk_status != 0) {
        return walk_status;
    }
    if ((flags & FTW_DEPTH) != 0) {
        const int result = function(path, &metadata, FTW_DP, &info);
        if (result != 0) {
            return result;
        }
    }
    return 0;
}

int nftw(const char *path,
         int (*function)(const char *, const struct stat *, int, struct FTW *),
         int descriptors, int flags)
{
    if (path == NULL || function == NULL) {
        errno = EINVAL;
        return -1;
    }
    /* `descriptors` is a hint for pre-opening directories; one opaque DIR at a
     * time is enough here, so it is accepted and ignored. */
    (void)descriptors;

    struct stat metadata;
    dev_t device = 0;
    if (stat(path, &metadata) == 0) {
        device = metadata.st_dev;
    }
    return nftw_walk(path, strlen(path), function, flags, 0, device);
}

/* nftw has no user pointer, so ftw parks its callback in a file-scope slot the
 * way glibc does. ftw is therefore not reentrant; nftw is the entry point for
 * callers that need nesting. The legacy callback has no type argument for
 * FTW_DP/FTW_SLN, so those collapse to the pre-order and file types. */
static int (*ftw_function)(const char *, const struct stat *, int);

static int ftw_bridge(const char *path, const struct stat *metadata, int type,
                      struct FTW *info)
{
    (void)info;
    int reported = type;
    if (type == FTW_DP) {
        reported = FTW_D;
    } else if (type == FTW_SLN) {
        reported = FTW_F;
    }
    return ftw_function(path, metadata, reported);
}

int ftw(const char *path, int (*function)(const char *, const struct stat *, int),
        int descriptors)
{
    if (path == NULL || function == NULL) {
        errno = EINVAL;
        return -1;
    }
    ftw_function = function;
    const int result =
        nftw(path, ftw_bridge, descriptors, FTW_DEPTH | FTW_PHYS);
    ftw_function = NULL;
    return result;
}

/* ---- popen/pclose ------------------------------------------------------- */

/* pclose has to recover the child from the stream pointer, and the API has no
 * other channel for it, so the association lives in this fixed table. The
 * table is small on purpose: a tool that runs hundreds of simultaneous
 * pipelines is not the target, and a fixed bound keeps this allocation-free. */
#define POPEN_SLOTS 16

struct popen_child {
    FILE *stream;
    pid_t child;
    int active;
};

static struct popen_child popen_children[POPEN_SLOTS];

FILE *popen(const char *command, const char *mode)
{
    if (command == NULL || mode == NULL ||
        (mode[0] != 'r' && mode[0] != 'w') ||
        (mode[1] != '\0' && !(mode[1] == 'e' && mode[2] == '\0'))) {
        errno = EINVAL;
        return NULL;
    }
    const int reading = mode[0] == 'r';

    int descriptors[2];
    if (pipe2(descriptors, O_CLOEXEC) < 0) {
        return NULL;
    }

    const pid_t child = fork();
    if (child < 0) {
        const int error = errno;
        close(descriptors[0]);
        close(descriptors[1]);
        errno = error;
        return NULL;
    }

    if (child == 0) {
        /* switch the pipe end onto the child's stdio, then run the shell. */
        if (reading) {
            close(descriptors[0]);
            if (dup2(descriptors[1], STDOUT_FILENO) < 0) {
                _exit(127);
            }
        } else {
            close(descriptors[1]);
            if (dup2(descriptors[0], STDIN_FILENO) < 0) {
                _exit(127);
            }
        }
        char *const arguments[] = {"sh", "-c", (char *)(uintptr_t)command, NULL};
        execvp("/bin/sh", arguments);
        _exit(127);
    }

    const int parent_descriptor = reading ? descriptors[0] : descriptors[1];
    close(reading ? descriptors[1] : descriptors[0]);

    FILE *stream = fdopen(parent_descriptor, reading ? "r" : "w");
    if (stream == NULL) {
        const int error = errno;
        close(parent_descriptor);
        errno = error;
        return NULL;
    }

    for (size_t slot = 0; slot < POPEN_SLOTS; ++slot) {
        if (popen_children[slot].active == 0) {
            popen_children[slot].stream = stream;
            popen_children[slot].child = child;
            popen_children[slot].active = 1;
            return stream;
        }
    }

    fclose(stream);
    errno = EMFILE;
    return NULL;
}

int pclose(FILE *stream)
{
    if (stream == NULL) {
        errno = EINVAL;
        return -1;
    }
    size_t slot = POPEN_SLOTS;
    for (size_t index = 0; index < POPEN_SLOTS; ++index) {
        if (popen_children[index].active != 0 &&
            popen_children[index].stream == stream) {
            slot = index;
            break;
        }
    }
    if (slot == POPEN_SLOTS) {
        errno = ECHILD;
        return -1;
    }

    const pid_t child = popen_children[slot].child;
    popen_children[slot].active = 0;
    popen_children[slot].stream = NULL;
    fclose(stream);

    int status = 0;
    for (;;) {
        const pid_t waited = waitpid(child, &status, 0);
        if (waited == child) {
            return status;
        }
        if (waited < 0 && errno == EINTR) {
            continue;
        }
        return -1;
    }
}

/* ---- arc4random --------------------------------------------------------- */

/* The kernel's getrandom is the source. When it is unavailable the state falls
 * back to a splitmix64 stream seeded from the clock and pid: weaker than the
 * kernel pool, but never a repeating constant, and the fallback is documented
 * here rather than silently pretending to be cryptographic. */
static uint64_t arc4random_fallback_state;

static uint64_t arc4random_next(void)
{
    uint64_t state = __atomic_load_n(&arc4random_fallback_state, __ATOMIC_RELAXED);
    if (state == 0) {
        struct timespec now;
        memset(&now, 0, sizeof(now));
        (void)clock_gettime(CLOCK_MONOTONIC, &now);
        state = ((uint64_t)(uint32_t)getpid() << 32) ^ (uint64_t)now.tv_nsec ^
                ((uint64_t)now.tv_sec * UINT64_C(0x2545f4914f6cdd1d));
        if (state == 0) {
            state = UINT64_C(0x9e3779b97f4a7c15);
        }
    }
    state += UINT64_C(0x9e3779b97f4a7c15);
    uint64_t mixed = state;
    mixed = (mixed ^ (mixed >> 30)) * UINT64_C(0xbf58476d1ce4e5b9);
    mixed = (mixed ^ (mixed >> 27)) * UINT64_C(0x94d049bb133111eb);
    mixed ^= mixed >> 31;
    __atomic_store_n(&arc4random_fallback_state, state, __ATOMIC_RELAXED);
    return mixed;
}

void arc4random_buf(void *buffer, size_t length)
{
    if (buffer == NULL || length == 0) {
        return;
    }
    unsigned char *bytes = buffer;
    size_t filled = 0;
    while (filled < length) {
        const ssize_t got = getrandom(bytes + filled, length - filled, 0);
        if (got > 0) {
            filled += (size_t)got;
            continue;
        }
        if (got < 0 && errno == EINTR) {
            continue;
        }
        const uint64_t value = arc4random_next();
        for (size_t index = 0; index < 8 && filled < length; ++index) {
            bytes[filled++] = (unsigned char)(value >> (8 * index));
        }
    }
}

uint32_t arc4random(void)
{
    uint32_t value = 0;
    arc4random_buf(&value, sizeof(value));
    return value;
}

uint32_t arc4random_uniform(uint32_t upper_bound)
{
    if (upper_bound < 2) {
        return 0;
    }
    /* Reject the biased head of the range, then take the modulo. */
    const uint32_t minimum = (uint32_t)(-upper_bound) % upper_bound;
    for (;;) {
        const uint32_t value = arc4random();
        if (value >= minimum) {
            return value % upper_bound;
        }
    }
}

/* ---- hsearch ------------------------------------------------------------ */

/* struct hsearch_data comes from <search.h>; only the node layout is private. */
struct hsearch_node {
    ENTRY entry;
    struct hsearch_node *next;
};

/* Chained hash table. Keys are borrowed: hdestroy frees the buckets and nodes
 * but not the caller's key strings, which is what glibc does as well. */
static struct hsearch_data global_hsearch_table;
static int global_hsearch_active;

static uint64_t hsearch_hash(const char *key)
{
    uint64_t hash = UINT64_C(0xcbf29ce484222325);
    for (const unsigned char *cursor = (const unsigned char *)key;
         *cursor != '\0'; ++cursor) {
        hash ^= *cursor;
        hash *= UINT64_C(0x100000001b3);
    }
    return hash;
}

static void hsearch_table_release(struct hsearch_data *table)
{
    if (table->buckets == NULL) {
        return;
    }
    for (size_t slot = 0; slot < table->capacity; ++slot) {
        struct hsearch_node *node = table->buckets[slot];
        while (node != NULL) {
            struct hsearch_node *next = node->next;
            free(node);
            node = next;
        }
    }
    free(table->buckets);
    table->buckets = NULL;
    table->capacity = 0;
    table->count = 0;
}

static int hsearch_table_grow(struct hsearch_data *table)
{
    const size_t capacity = table->capacity != 0 ? table->capacity * 2 : 16;
    struct hsearch_node **buckets = calloc(capacity, sizeof(*buckets));
    if (buckets == NULL) {
        return -1;
    }
    for (size_t slot = 0; slot < table->capacity; ++slot) {
        struct hsearch_node *node = table->buckets[slot];
        while (node != NULL) {
            struct hsearch_node *next = node->next;
            const size_t target =
                (size_t)(hsearch_hash(node->entry.key) & (capacity - 1));
            node->next = buckets[target];
            buckets[target] = node;
            node = next;
        }
    }
    free(table->buckets);
    table->buckets = buckets;
    table->capacity = capacity;
    return 0;
}

static ENTRY *hsearch_lookup(struct hsearch_data *table, ENTRY item,
                            ACTION action)
{
    if (table->capacity == 0 && hsearch_table_grow(table) < 0) {
        return NULL;
    }
    for (;;) {
        const size_t slot =
            (size_t)(hsearch_hash(item.key) & (table->capacity - 1));
        for (struct hsearch_node *node = table->buckets[slot]; node != NULL;
             node = node->next) {
            if (strcmp(node->entry.key, item.key) == 0) {
                return &node->entry;
            }
        }
        if (action == FIND) {
            return NULL;
        }
        if (table->count * 2 >= table->capacity && hsearch_table_grow(table) < 0) {
            return NULL;
        }
        struct hsearch_node *node = malloc(sizeof(*node));
        if (node == NULL) {
            return NULL;
        }
        node->entry = item;
        node->next = table->buckets[slot];
        table->buckets[slot] = node;
        table->count++;
        return &node->entry;
    }
}

int hcreate(size_t element_count)
{
    if (global_hsearch_active != 0) {
        return 0;
    }
    memset(&global_hsearch_table, 0, sizeof(global_hsearch_table));
    if (element_count > SIZE_MAX / 2) {
        errno = ENOMEM;
        return 0;
    }
    while (global_hsearch_table.capacity < element_count * 2) {
        if (hsearch_table_grow(&global_hsearch_table) < 0) {
            hsearch_table_release(&global_hsearch_table);
            errno = ENOMEM;
            return 0;
        }
    }
    global_hsearch_active = 1;
    return 1;
}

void hdestroy(void)
{
    hsearch_table_release(&global_hsearch_table);
    global_hsearch_active = 0;
}

ENTRY *hsearch(ENTRY item, ACTION action)
{
    if (item.key == NULL) {
        errno = EINVAL;
        return NULL;
    }
    if (global_hsearch_active == 0) {
        errno = EINVAL;
        return NULL;
    }
    return hsearch_lookup(&global_hsearch_table, item, action);
}

int hcreate_r(size_t element_count, struct hsearch_data *table)
{
    if (table == NULL) {
        errno = EINVAL;
        return 0;
    }
    memset(table, 0, sizeof(*table));
    while (table->capacity < element_count * 2) {
        if (hsearch_table_grow(table) < 0) {
            hsearch_table_release(table);
            errno = ENOMEM;
            return 0;
        }
    }
    return 1;
}

void hdestroy_r(struct hsearch_data *table)
{
    if (table != NULL) {
        hsearch_table_release(table);
    }
}

int hsearch_r(ENTRY item, ACTION action, ENTRY **result,
              struct hsearch_data *table)
{
    if (table == NULL || result == NULL) {
        errno = EINVAL;
        return 0;
    }
    *result = NULL;
    if (item.key == NULL) {
        errno = EINVAL;
        return 0;
    }
    ENTRY *found = hsearch_lookup(table, item, action);
    if (found == NULL) {
        return 0;
    }
    *result = found;
    return 1;
}

/* ---- strptime ----------------------------------------------------------- */

static const char *const month_names[] = {"january", "february", "march",
                                          "april",   "may",      "june",
                                          "july",    "august",   "september",
                                          "october", "november", "december"};

static const char *const month_abbreviations[] = {"jan", "feb", "mar", "apr",
                                                  "may", "jun", "jul", "aug",
                                                  "sep", "oct", "nov", "dec"};

static const char *const day_names[] = {"sunday",   "monday", "tuesday",
                                        "wednesday", "thursday", "friday",
                                        "saturday"};

static const char *const day_abbreviations[] = {"sun", "mon", "tue", "wed",
                                                "thu", "fri", "sat"};

static int match_name(const char **cursor, const char *text)
{
    const size_t length = strlen(text);
    for (size_t index = 0; index < length; ++index) {
        if (cursor[0][index] == '\0') {
            return 0;
        }
        if (tolower((unsigned char)cursor[0][index]) !=
            (unsigned char)text[index]) {
            return 0;
        }
    }
    *cursor += length;
    return 1;
}

static int parse_digits(const char **cursor, int minimum, int maximum, int *value)
{
    int digits = 0;
    int result = 0;
    while (digits < maximum && isdigit((unsigned char)cursor[0][0])) {
        result = result * 10 + (cursor[0][0] - '0');
        ++cursor[0];
        ++digits;
    }
    if (digits < minimum) {
        return 0;
    }
    *value = result;
    return 1;
}

static void skip_whitespace(const char **cursor)
{
    while (isspace((unsigned char)cursor[0][0])) {
        ++cursor[0];
    }
}

/* Parses the time/date formats a build log or HTTP header actually carries. The
 * weekday and zone names are validated but not stored: struct tm has no field
 * for them, and DST determination is left to mktime. */
char *strptime(const char *input, const char *format, struct tm *value)
{
    if (input == NULL || format == NULL || value == NULL) {
        errno = EINVAL;
        return NULL;
    }

    const char *cursor = input;
    for (const char *spec = format; *spec != '\0'; ++spec) {
        if (*spec != '%') {
            if (isspace((unsigned char)*spec)) {
                skip_whitespace(&cursor);
                continue;
            }
            if (*cursor != *spec) {
                return NULL;
            }
            ++cursor;
            continue;
        }

        ++spec;
        const char conversion = *spec;
        int number = 0;
        switch (conversion) {
        case '\0':
            return NULL;
        case '%':
            if (*cursor != '%') {
                return NULL;
            }
            ++cursor;
            break;
        case 'n':
        case 't':
            skip_whitespace(&cursor);
            break;
        case 'a':
        case 'A': {
            int matched = 0;
            for (int index = 0; index < 7 && matched == 0; ++index) {
                const char *probe = cursor;
                if (match_name(&probe, day_abbreviations[index]) != 0 ||
                    match_name(&probe, day_names[index]) != 0) {
                    cursor = probe;
                    value->tm_wday = index;
                    matched = 1;
                }
            }
            if (matched == 0) {
                return NULL;
            }
            break;
        }
        case 'b':
        case 'B':
        case 'h': {
            int matched = 0;
            for (int index = 0; index < 12 && matched == 0; ++index) {
                const char *probe = cursor;
                if (match_name(&probe, month_abbreviations[index]) != 0 ||
                    match_name(&probe, month_names[index]) != 0) {
                    cursor = probe;
                    value->tm_mon = index;
                    matched = 1;
                }
            }
            if (matched == 0) {
                return NULL;
            }
            break;
        }
        case 'C':
            if (parse_digits(&cursor, 1, 2, &number) == 0) {
                return NULL;
            }
            value->tm_year = (number * 100) - 1900;
            break;
        case 'd':
        case 'e':
            skip_whitespace(&cursor);
            if (parse_digits(&cursor, 1, 2, &number) == 0 || number < 1 ||
                number > 31) {
                return NULL;
            }
            value->tm_mday = number;
            break;
        case 'H':
        case 'k':
            skip_whitespace(&cursor);
            if (parse_digits(&cursor, 1, 2, &number) == 0 || number > 23) {
                return NULL;
            }
            value->tm_hour = number;
            break;
        case 'I':
        case 'l':
            skip_whitespace(&cursor);
            if (parse_digits(&cursor, 1, 2, &number) == 0 || number < 1 ||
                number > 12) {
                return NULL;
            }
            value->tm_hour = number % 12;
            break;
        case 'j':
            if (parse_digits(&cursor, 1, 3, &number) == 0 || number < 1 ||
                number > 366) {
                return NULL;
            }
            value->tm_yday = number - 1;
            break;
        case 'm':
            if (parse_digits(&cursor, 1, 2, &number) == 0 || number < 1 ||
                number > 12) {
                return NULL;
            }
            value->tm_mon = number - 1;
            break;
        case 'M':
            if (parse_digits(&cursor, 1, 2, &number) == 0 || number > 59) {
                return NULL;
            }
            value->tm_min = number;
            break;
        case 'p':
        case 'P': {
            const char *probe = cursor;
            if (match_name(&probe, "am") != 0) {
                if (value->tm_hour == 12) {
                    value->tm_hour = 0;
                }
            } else if (match_name(&probe, "pm") != 0) {
                if (value->tm_hour < 12) {
                    value->tm_hour += 12;
                }
            } else {
                return NULL;
            }
            cursor = probe;
            break;
        }
        case 'S':
            if (parse_digits(&cursor, 1, 2, &number) == 0 || number > 60) {
                return NULL;
            }
            value->tm_sec = number;
            break;
        case 'y':
            if (parse_digits(&cursor, 1, 2, &number) == 0) {
                return NULL;
            }
            value->tm_year = number < 69 ? number + 100 : number;
            break;
        case 'Y':
            if (parse_digits(&cursor, 1, 4, &number) == 0) {
                return NULL;
            }
            value->tm_year = number - 1900;
            break;
        case 'u':
            if (parse_digits(&cursor, 1, 1, &number) == 0 || number < 1 ||
                number > 7) {
                return NULL;
            }
            value->tm_wday = number % 7;
            break;
        case 'w':
            if (parse_digits(&cursor, 1, 1, &number) == 0 || number > 6) {
                return NULL;
            }
            value->tm_wday = number;
            break;
        case 'U':
        case 'W':
        case 'V':
            if (parse_digits(&cursor, 1, 2, &number) == 0 || number > 53) {
                return NULL;
            }
            break;
        case 'Z': {
            /* Zone names are consumed but not interpreted; only %z carries a
             * numeric offset, and struct tm has no place for either. */
            while (isalpha((unsigned char)*cursor) || *cursor == '/' ||
                   *cursor == '+' || *cursor == '-') {
                ++cursor;
            }
            break;
        }
        case 'z':
            if (*cursor == 'Z') {
                ++cursor;
                break;
            }
            if (*cursor == '+' || *cursor == '-') {
                ++cursor;
            } else {
                return NULL;
            }
            if (parse_digits(&cursor, 2, 2, &number) == 0) {
                return NULL;
            }
            if (*cursor == ':') {
                ++cursor;
            }
            if (parse_digits(&cursor, 2, 2, &number) == 0) {
                return NULL;
            }
            break;
        case 'D': {
            const char *rest = strptime(cursor, "%m/%d/%y", value);
            if (rest == NULL) {
                return NULL;
            }
            cursor = rest;
            break;
        }
        case 'F': {
            const char *rest = strptime(cursor, "%Y-%m-%d", value);
            if (rest == NULL) {
                return NULL;
            }
            cursor = rest;
            break;
        }
        case 'R': {
            const char *rest = strptime(cursor, "%H:%M", value);
            if (rest == NULL) {
                return NULL;
            }
            cursor = rest;
            break;
        }
        case 'T':
        case 'X': {
            const char *rest = strptime(cursor, "%H:%M:%S", value);
            if (rest == NULL) {
                return NULL;
            }
            cursor = rest;
            break;
        }
        case 'r': {
            const char *rest = strptime(cursor, "%I:%M:%S %p", value);
            if (rest == NULL) {
                return NULL;
            }
            cursor = rest;
            break;
        }
        case 'x': {
            const char *rest = strptime(cursor, "%m/%d/%y", value);
            if (rest == NULL) {
                return NULL;
            }
            cursor = rest;
            break;
        }
        case 'c': {
            const char *rest = strptime(cursor, "%a %b %e %H:%M:%S %Y", value);
            if (rest == NULL) {
                return NULL;
            }
            cursor = rest;
            break;
        }
        default:
            /* Unknown conversions are rejected instead of silently skipped. */
            return NULL;
        }
    }
    return (char *)cursor;
}

/* ---- fts ---------------------------------------------------------------- */

/* A directory being walked. Children are materialised on first use so that a
 * caller-supplied comparison can order them; fts_children then just hands that
 * list over and fts_read resumes at the next sibling directory. */
struct fts_frame {
    FTSENT *entry;
    DIR *directory;
    FTSENT **children;
    size_t count;
    size_t index;
    int listed;
};

struct _fts {
    int options;
    int (*compar)(const FTSENT **, const FTSENT **);
    char **roots;
    size_t root_count;
    size_t root_index;
    struct fts_frame *frames;
    size_t depth;
    size_t frame_capacity;
    FTSENT **entries; /* everything allocated, released in fts_close */
    size_t entry_count;
    size_t entry_capacity;
    int root_device_set;
    dev_t root_device;
    FTSENT *current;
    int finished;
};

/* Every FTSENT is retained until fts_close: fts_read hands out pointers that
 * the caller may keep, and releasing an entry early would dangle them. */
static int fts_track(FTS *stream, FTSENT *entry)
{
    if (stream->entry_count == stream->entry_capacity) {
        const size_t capacity = stream->entry_capacity != 0
                                    ? stream->entry_capacity * 2
                                    : 32;
        FTSENT **grown = realloc(stream->entries, capacity * sizeof(*grown));
        if (grown == NULL) {
            errno = ENOMEM;
            return -1;
        }
        stream->entries = grown;
        stream->entry_capacity = capacity;
    }
    stream->entries[stream->entry_count++] = entry;
    return 0;
}

static void fts_entry_release(FTSENT *entry)
{
    if (entry == NULL) {
        return;
    }
    free(entry->fts_path);
    free(entry->fts_statp);
    free(entry);
}

static FTSENT *fts_entry_create(const char *parent_path, const char *name,
                                short level)
{
    const size_t name_length = strlen(name);
    FTSENT *entry = calloc(1, offsetof(FTSENT, fts_name) + name_length + 1);
    if (entry == NULL) {
        return NULL;
    }

    size_t parent_length = 0;
    size_t join = 0;
    if (parent_path != NULL) {
        parent_length = strlen(parent_path);
        if (parent_length != 0 && parent_path[parent_length - 1] != '/') {
            join = 1;
        }
    }
    const size_t path_length = parent_length + join + name_length;
    entry->fts_path = malloc(path_length + 1);
    if (entry->fts_path == NULL) {
        free(entry);
        return NULL;
    }
    if (parent_length != 0) {
        memcpy(entry->fts_path, parent_path, parent_length);
    }
    if (join != 0) {
        entry->fts_path[parent_length] = '/';
    }
    memcpy(entry->fts_path + parent_length + join, name, name_length + 1);

    memcpy(entry->fts_name, name, name_length + 1);
    entry->fts_namelen = (unsigned short)name_length;
    entry->fts_pathlen = (unsigned short)(path_length > 65535 ? 65535 : path_length);
    entry->fts_accpath = entry->fts_path;
    entry->fts_symfd = -1;
    entry->fts_level = level;
    entry->fts_info = FTS_INIT;
    return entry;
}

static unsigned short fts_classify(mode_t mode)
{
    if (S_ISDIR(mode)) {
        return FTS_D;
    }
    if (S_ISLNK(mode)) {
        return FTS_SL;
    }
    if (S_ISREG(mode)) {
        return FTS_F;
    }
    return FTS_DEFAULT;
}

static void fts_entry_stat(FTS *stream, FTSENT *entry, int is_root)
{
    struct stat metadata;
    const int follow = (stream->options & FTS_LOGICAL) != 0 ||
                       (is_root != 0 && (stream->options & FTS_COMFOLLOW) != 0);

    int status = follow ? stat(entry->fts_path, &metadata)
                        : lstat(entry->fts_path, &metadata);
    if (status < 0 && follow != 0) {
        /* A dangling symlink under FTS_LOGICAL is FTS_SLNONE, not FTS_NS. */
        struct stat link_metadata;
        if (lstat(entry->fts_path, &link_metadata) == 0 &&
            S_ISLNK(link_metadata.st_mode)) {
            metadata = link_metadata;
            status = 0;
            entry->fts_info = FTS_SLNONE;
        }
    }
    if (status < 0) {
        entry->fts_info = FTS_NS;
        entry->fts_errno = errno;
        return;
    }

    entry->fts_statp = malloc(sizeof(metadata));
    if (entry->fts_statp == NULL) {
        entry->fts_info = FTS_ERR;
        entry->fts_errno = ENOMEM;
        return;
    }
    *entry->fts_statp = metadata;
    entry->fts_ino = metadata.st_ino;
    entry->fts_dev = metadata.st_dev;
    entry->fts_nlink = metadata.st_nlink;
    if (entry->fts_info != FTS_SLNONE) {
        entry->fts_info = fts_classify(metadata.st_mode);
    }

    if (is_root != 0 && stream->root_device_set == 0) {
        stream->root_device = metadata.st_dev;
        stream->root_device_set = 1;
    }
}

static FTSENT *fts_find_cycle(FTS *stream, const FTSENT *entry)
{
    for (size_t index = stream->depth; index > 0; --index) {
        FTSENT *ancestor = stream->frames[index - 1].entry;
        if (ancestor != NULL && ancestor->fts_statp != NULL &&
            ancestor->fts_ino == entry->fts_ino &&
            ancestor->fts_dev == entry->fts_dev) {
            return ancestor;
        }
    }
    return NULL;
}

static int fts_push_directory(FTS *stream, FTSENT *entry)
{
    if ((stream->options & FTS_XDEV) != 0 && stream->root_device_set != 0 &&
        entry->fts_dev != stream->root_device) {
        /* Same filesystem only: report the directory but do not descend. */
        return 0;
    }
    DIR *directory = opendir(entry->fts_path);
    if (directory == NULL) {
        entry->fts_info = FTS_DNR;
        entry->fts_errno = errno;
        return -1;
    }
    if (stream->depth == stream->frame_capacity) {
        const size_t capacity =
            stream->frame_capacity != 0 ? stream->frame_capacity * 2 : 16;
        struct fts_frame *grown =
            realloc(stream->frames, capacity * sizeof(*grown));
        if (grown == NULL) {
            closedir(directory);
            entry->fts_info = FTS_ERR;
            entry->fts_errno = ENOMEM;
            errno = ENOMEM;
            return -1;
        }
        stream->frames = grown;
        stream->frame_capacity = capacity;
    }
    struct fts_frame *frame = &stream->frames[stream->depth++];
    memset(frame, 0, sizeof(*frame));
    frame->entry = entry;
    frame->directory = directory;
    return 0;
}

static void fts_frame_release(struct fts_frame *frame)
{
    if (frame->directory != NULL) {
        closedir(frame->directory);
        frame->directory = NULL;
    }
    free(frame->children);
    frame->children = NULL;
    frame->count = 0;
    frame->index = 0;
    frame->listed = 0;
}

static int fts_frame_materialize(FTS *stream, struct fts_frame *frame)
{
    frame->listed = 1;
    if (frame->directory == NULL) {
        return 0;
    }

    size_t capacity = 8;
    size_t count = 0;
    FTSENT **children = malloc(capacity * sizeof(*children));
    if (children == NULL) {
        errno = ENOMEM;
        return -1;
    }

    const short child_level = (short)(frame->entry->fts_level + 1);
    for (;;) {
        errno = 0;
        struct dirent *dirent = readdir(frame->directory);
        if (dirent == NULL) {
            if (errno != 0) {
                const int error = errno;
                free(children);
                closedir(frame->directory);
                frame->directory = NULL;
                errno = error;
                return -1;
            }
            break;
        }
        const char *name = dirent->d_name;
        if ((stream->options & FTS_SEEDOT) == 0 &&
            (strcmp(name, ".") == 0 || strcmp(name, "..") == 0)) {
            continue;
        }
        if (count == capacity) {
            const size_t grown_capacity = capacity * 2;
            FTSENT **grown =
                realloc(children, grown_capacity * sizeof(*children));
            if (grown == NULL) {
                free(children);
                errno = ENOMEM;
                return -1;
            }
            children = grown;
            capacity = grown_capacity;
        }
        FTSENT *child =
            fts_entry_create(frame->entry->fts_path, name, child_level);
        if (child == NULL || fts_track(stream, child) < 0) {
            fts_entry_release(child);
            free(children);
            errno = ENOMEM;
            return -1;
        }
        child->fts_parent = frame->entry;
        fts_entry_stat(stream, child, 0);
        if (child->fts_info == FTS_D) {
            FTSENT *cycle = fts_find_cycle(stream, child);
            if (cycle != NULL) {
                child->fts_info = FTS_DC;
                child->fts_cycle = cycle;
            }
        }
        children[count++] = child;
    }

    closedir(frame->directory);
    frame->directory = NULL;

    if (stream->compar != NULL) {
        for (size_t index = 1; index < count; ++index) {
            FTSENT *current = children[index];
            size_t position = index;
            while (position > 0 &&
                   stream->compar((const FTSENT **)&children[position - 1],
                                  (const FTSENT **)&current) > 0) {
                children[position] = children[position - 1];
                --position;
            }
            children[position] = current;
        }
    }
    for (size_t index = 0; index < count; ++index) {
        children[index]->fts_link =
            index + 1 < count ? children[index + 1] : NULL;
    }

    frame->children = children;
    frame->count = count;
    frame->index = 0;
    return 0;
}

FTS *fts_open(char *const *path_argv, int options,
              int (*compar)(const FTSENT **, const FTSENT **))
{
    if (path_argv == NULL || path_argv[0] == NULL) {
        errno = EINVAL;
        return NULL;
    }
    FTS *stream = calloc(1, sizeof(*stream));
    if (stream == NULL) {
        errno = ENOMEM;
        return NULL;
    }
    stream->options = options;
    stream->compar = compar;

    size_t count = 0;
    while (path_argv[count] != NULL) {
        ++count;
    }
    stream->roots = calloc(count, sizeof(*stream->roots));
    if (stream->roots == NULL) {
        free(stream);
        errno = ENOMEM;
        return NULL;
    }
    for (size_t index = 0; index < count; ++index) {
        stream->roots[index] = strdup(path_argv[index]);
        if (stream->roots[index] == NULL) {
            stream->root_count = index;
            fts_close(stream);
            errno = ENOMEM;
            return NULL;
        }
        stream->root_count = index + 1;
    }
    return stream;
}

FTSENT *fts_read(FTS *stream)
{
    if (stream == NULL) {
        errno = EINVAL;
        return NULL;
    }
    if (stream->finished != 0) {
        errno = 0;
        return NULL;
    }

    if (stream->depth > 0) {
        struct fts_frame *frame = &stream->frames[stream->depth - 1];
        if (frame->listed == 0 && fts_frame_materialize(stream, frame) < 0) {
            stream->finished = 1;
            return NULL;
        }
        if (frame->index < frame->count) {
            FTSENT *entry = frame->children[frame->index++];
            if (entry->fts_info == FTS_D) {
                (void)fts_push_directory(stream, entry);
            }
            stream->current = entry;
            errno = 0;
            return entry;
        }
        FTSENT *directory = frame->entry;
        fts_frame_release(frame);
        stream->depth--;
        directory->fts_info = FTS_DP;
        stream->current = directory;
        errno = 0;
        return directory;
    }

    if (stream->root_index < stream->root_count) {
        const char *path = stream->roots[stream->root_index++];
        FTSENT *entry = fts_entry_create(NULL, path, 0);
        if (entry == NULL || fts_track(stream, entry) < 0) {
            fts_entry_release(entry);
            stream->finished = 1;
            errno = ENOMEM;
            return NULL;
        }
        fts_entry_stat(stream, entry, 1);
        if (entry->fts_info == FTS_D) {
            (void)fts_push_directory(stream, entry);
        }
        stream->current = entry;
        errno = 0;
        return entry;
    }

    stream->finished = 1;
    errno = 0;
    return NULL;
}

FTSENT *fts_children(FTS *stream, int instructions)
{
    (void)instructions;
    if (stream == NULL) {
        errno = EINVAL;
        return NULL;
    }
    if (stream->depth == 0) {
        errno = 0;
        return NULL;
    }
    struct fts_frame *frame = &stream->frames[stream->depth - 1];
    if (frame->entry == NULL || stream->current != frame->entry) {
        errno = 0;
        return NULL;
    }
    if (frame->listed == 0) {
        if (fts_frame_materialize(stream, frame) < 0) {
            errno = 0;
            return NULL;
        }
        /* The caller now owns the traversal of these entries. */
        frame->index = frame->count;
    }
    errno = 0;
    return frame->count != 0 ? frame->children[0] : NULL;
}

int fts_set(FTS *stream, FTSENT *entry, int instructions)
{
    if (stream == NULL || entry == NULL) {
        errno = EINVAL;
        return -1;
    }
    if (instructions == FTS_SKIP) {
        if (stream->depth > 0) {
            struct fts_frame *frame = &stream->frames[stream->depth - 1];
            if (frame->entry == entry) {
                /*
                 * fts_read() has opened the directory but has not necessarily
                 * materialized its children yet. Materialize first so the
                 * skip index cannot be reset to zero by the next fts_read().
                 */
                if (frame->listed == 0 &&
                    fts_frame_materialize(stream, frame) < 0) {
                    return -1;
                }
                frame->index = frame->count;
            }
        }
        return 0;
    }
    if (instructions == FTS_CHDIRED || instructions == FTS_DOT) {
        /* There is no chdir mode, so both are no-ops here. */
        return 0;
    }
    errno = EINVAL;
    return -1;
}

int fts_close(FTS *stream)
{
    if (stream == NULL) {
        errno = EINVAL;
        return -1;
    }
    while (stream->depth > 0) {
        --stream->depth;
        fts_frame_release(&stream->frames[stream->depth]);
    }
    free(stream->frames);
    for (size_t index = 0; index < stream->entry_count; ++index) {
        fts_entry_release(stream->entries[index]);
    }
    free(stream->entries);
    for (size_t index = 0; index < stream->root_count; ++index) {
        free(stream->roots[index]);
    }
    free(stream->roots);
    free(stream);
    return 0;
}

/* ---- execvpe ------------------------------------------------------------ */

/* execvp with a caller-supplied environment. The PATH search follows the same
 * rules as the existing execvp: an empty element means the current directory,
 * and EACCES keeps being remembered in case a later candidate works. */
int execvpe(const char *file, char *const argv[], char *const environment[])
{
    if (file == NULL || file[0] == '\0') {
        errno = ENOENT;
        return -1;
    }
    if (strchr(file, '/') != NULL) {
        return execve(file, argv, environment);
    }

    const char *path = getenv("PATH");
    if (path == NULL) {
        path = "/bin:/usr/bin";
    }

    int saw_eacces = 0;
    const char *cursor = path;
    for (;;) {
        const char *colon = strchr(cursor, ':');
        const size_t length = colon != NULL ? (size_t)(colon - cursor) : strlen(cursor);
        const size_t needed = length + (length != 0 ? 1 : 0) + strlen(file) + 1;
        char *candidate = malloc(needed);
        if (candidate == NULL) {
            errno = ENOMEM;
            return -1;
        }
        size_t position = 0;
        if (length != 0) {
            memcpy(candidate, cursor, length);
            position = length;
            candidate[position++] = '/';
        }
        strcpy(candidate + position, file);
        (void)execve(candidate, argv, environment);
        const int error = errno;
        free(candidate);
        if (error == EACCES) {
            saw_eacces = 1;
        } else if (error != ENOENT && error != ENOTDIR) {
            errno = error;
            return -1;
        }
        if (colon == NULL) {
            break;
        }
        cursor = colon + 1;
    }

    errno = saw_eacces != 0 ? EACCES : ENOENT;
    return -1;
}

/* ---- posix_fallocate ---------------------------------------------------- */

/* There is no fallocate syscall: extend with ftruncate and touch the last byte
 * so the blocks really exist, which is the portable implementation POSIX
 * allows. The return value is an error number, not -1. */
int posix_fallocate(int descriptor, off_t offset, off_t length)
{
    if (descriptor < 0 || offset < 0 || length <= 0) {
        return EINVAL;
    }
    const off_t end = offset + length;
    if (end < offset) {
        return EOVERFLOW;
    }
    struct stat metadata;
    if (fstat(descriptor, &metadata) < 0) {
        return errno;
    }
    if (metadata.st_size >= end) {
        return 0;
    }
    if (ftruncate(descriptor, end) < 0) {
        return errno;
    }
    const char zero = 0;
    if (pwrite(descriptor, &zero, 1, end - 1) != 1) {
        return errno != 0 ? errno : EIO;
    }
    return 0;
}

/* ---- thread names ------------------------------------------------------- */

/* The kernel has no per-thread name, so both calls report ENOSYS instead of
 * pretending the name was stored somewhere. */
int pthread_setname_np(pthread_t thread, const char *name)
{
    (void)thread;
    (void)name;
    return ENOSYS;
}

int pthread_getname_np(pthread_t thread, char *name, size_t length)
{
    (void)thread;
    (void)name;
    (void)length;
    return ENOSYS;
}

/* ---- POSIX shared memory objects --------------------------------------- */

/* OCTAGRAM does not yet have a kernel-backed named shared-memory namespace.
 * Keep the standard API linkable, but report ENOSYS rather than emulating it
 * with ordinary files (which would not provide the required unlink/lifetime
 * semantics across processes).
 */
static int octagram_valid_shm_name(const char *name)
{
    return name != NULL && name[0] == '/' && name[1] != '\0' &&
           strchr(name + 1, '/') == NULL;
}

int shm_open(const char *name, int flags, mode_t mode)
{
    (void)flags;
    (void)mode;
    if (!octagram_valid_shm_name(name)) {
        errno = EINVAL;
        return -1;
    }
    errno = ENOSYS;
    return -1;
}

int shm_unlink(const char *name)
{
    if (!octagram_valid_shm_name(name)) {
        errno = EINVAL;
        return -1;
    }
    errno = ENOSYS;
    return -1;
}

/* ---- mmap64 ------------------------------------------------------------- */

/* OCTAGRAM's off_t is already 64-bit, so mmap64 is the same call. It exists for
 * code that selects the LFS name at compile time. */
void *mmap64(void *address, size_t length, int protection, int flags,
             int file_descriptor, off_t offset)
{
    return mmap(address, length, protection, flags, file_descriptor, offset);
}

/* ---- Allocator helpers -------------------------------------------------- */

int getpagesize(void)
{
    const long page = sysconf(_SC_PAGESIZE);
    return page > 0 ? (int)page : 4096;
}

void *memalign(size_t alignment, size_t size)
{
    if (alignment < sizeof(void *)) {
        alignment = sizeof(void *);
    }
    if ((alignment & (alignment - 1)) != 0) {
        errno = EINVAL;
        return NULL;
    }
    void *pointer = NULL;
    const int status = posix_memalign(&pointer, alignment, size);
    if (status != 0) {
        errno = status;
        return NULL;
    }
    return pointer;
}

void *valloc(size_t size)
{
    return memalign((size_t)getpagesize(), size);
}

/* pvalloc rounds the request up to whole pages, and a zero size asks for one
 * page, which is the glibc behaviour. */
void *pvalloc(size_t size)
{
    const size_t page = (size_t)getpagesize();
    if (size == 0) {
        size = page;
    }
    if (size > SIZE_MAX - page) {
        errno = ENOMEM;
        return NULL;
    }
    const size_t rounded = (size + page - 1) & ~(page - 1);
    return memalign(page, rounded);
}

void cfree(void *pointer)
{
    free(pointer);
}

int mallopt(int parameter, int value)
{
    (void)parameter;
    (void)value;
    /* No thresholds exist in this allocator; report "not applied" rather than
     * claiming the tuning took effect. */
    return 0;
}

int malloc_trim(size_t pad)
{
    (void)pad;
    return 0;
}

void malloc_stats(void)
{
    static const char message[] =
        "OCTAGRAM allocator: per-arena statistics are not tracked\n";
    (void)write(STDERR_FILENO, message, sizeof(message) - 1);
}

/* ---- Named semaphores ---------------------------------------------------- */
/* The counting semaphore (sem_t, sem_wait/sem_post) is implemented over a futex
 * in runtime.c; only the named form is new here. */


#define SEMAPHORE_NAMED_SLOTS 8

struct octagram_named_semaphore {
    char name[64];
    sem_t semaphore;
    int active;
};

static struct octagram_named_semaphore octagram_named_semaphores[SEMAPHORE_NAMED_SLOTS];

sem_t *sem_open(const char *name, int flags, ...)
{
    if (name == NULL || name[0] == '\0') {
        errno = EINVAL;
        return SEM_FAILED;
    }

    unsigned int value = 0;
    if ((flags & O_CREAT) != 0) {
        va_list arguments;
        va_start(arguments, flags);
        (void)va_arg(arguments, int); /* mode_t, unused: nothing touches disk */
        value = va_arg(arguments, unsigned int);
        va_end(arguments);
    }

    for (size_t slot = 0; slot < SEMAPHORE_NAMED_SLOTS; ++slot) {
        struct octagram_named_semaphore *entry = &octagram_named_semaphores[slot];
        if (entry->active != 0 && strcmp(entry->name, name) == 0) {
            if ((flags & O_EXCL) != 0) {
                errno = EEXIST;
                return SEM_FAILED;
            }
            return &entry->semaphore;
        }
    }
    if ((flags & O_CREAT) == 0) {
        errno = ENOENT;
        return SEM_FAILED;
    }
    for (size_t slot = 0; slot < SEMAPHORE_NAMED_SLOTS; ++slot) {
        struct octagram_named_semaphore *entry = &octagram_named_semaphores[slot];
        if (entry->active != 0) {
            continue;
        }
        const size_t length = strlen(name);
        if (length + 1 > sizeof(entry->name)) {
            errno = ENAMETOOLONG;
            return SEM_FAILED;
        }
        memcpy(entry->name, name, length + 1);
        (void)sem_init(&entry->semaphore, 0, value);
        entry->active = 1;
        return &entry->semaphore;
    }
    errno = ENFILE;
    return SEM_FAILED;
}

int sem_close(sem_t *semaphore)
{
    if (semaphore == NULL) {
        errno = EINVAL;
        return -1;
    }
    for (size_t slot = 0; slot < SEMAPHORE_NAMED_SLOTS; ++slot) {
        if (octagram_named_semaphores[slot].active != 0 &&
            &octagram_named_semaphores[slot].semaphore == semaphore) {
            octagram_named_semaphores[slot].active = 0;
            return 0;
        }
    }
    /* Unnamed semaphores are closed by sem_destroy, not sem_close. */
    errno = EINVAL;
    return -1;
}

int sem_unlink(const char *name)
{
    if (name == NULL) {
        errno = EINVAL;
        return -1;
    }
    for (size_t slot = 0; slot < SEMAPHORE_NAMED_SLOTS; ++slot) {
        struct octagram_named_semaphore *entry = &octagram_named_semaphores[slot];
        if (entry->active != 0 && strcmp(entry->name, name) == 0) {
            entry->active = 0;
            return 0;
        }
    }
    errno = ENOENT;
    return -1;
}
