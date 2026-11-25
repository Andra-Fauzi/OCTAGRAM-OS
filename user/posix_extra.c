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
#include <err.h>
#include <errno.h>
#include <error.h>
#include <fnmatch.h>
#include <getopt.h>
#include <glob.h>
#include <sched.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>
#include <sys/mman.h>
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
