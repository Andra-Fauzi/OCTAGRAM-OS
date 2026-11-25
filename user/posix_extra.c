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

#include <dirent.h>
#include <errno.h>
#include <fnmatch.h>
#include <glob.h>
#include <sched.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>
#include <sys/stat.h>
#include <sys/sysinfo.h>
#include <time.h>
#include <unistd.h>

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
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
            continue;
        }
        if (fnmatch(component, entry->d_name, 0) != 0) {
            continue;
        }
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
        status = oct_glob_walk(child, rest, result);
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

    status = oct_glob_walk(prefix, pattern_start, result);
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
        qsort(result->gl_pathv + existing, result->gl_pathc - existing,
              sizeof(char *), oct_glob_compare);
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
