/*
 * Wide-character string and conversion helpers required by the C99 <wchar.h>
 * API. libc++'s <cwchar> wrapper re-declares every one of these in namespace
 * std, so they have to exist for the C++ standard headers to compile.
 *
 * The OCTAGRAM locale is ASCII/Latin-1: one byte is one wide character, and no
 * shift state is ever needed. Buffer formatting (swprintf/vswprintf) is
 * implemented at the end of this file because libc++'s <string> numeric
 * helpers call it. Stream-based wide stdio (fgetwc, wprintf, fwprintf, ...) is
 * not implemented yet; those entry points are declared but have no definition,
 * so using one fails at link time instead of silently misbehaving.
 */
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

wchar_t *wcscat(wchar_t *destination, const wchar_t *source)
{
    wcscpy(destination + wcslen(destination), source);
    return destination;
}

wchar_t *wcsncat(wchar_t *destination, const wchar_t *source, size_t count)
{
    wchar_t *tail = destination + wcslen(destination);
    size_t index = 0;
    while (index < count && source[index] != L'\0') {
        tail[index] = source[index];
        ++index;
    }
    tail[index] = L'\0';
    return destination;
}



wchar_t *wcsstr(const wchar_t *text, const wchar_t *needle)
{
    if (needle[0] == L'\0') {
        return (wchar_t *)text;
    }
    for (const wchar_t *start = text; *start != L'\0'; ++start) {
        size_t index = 0;
        while (needle[index] != L'\0' && start[index] == needle[index]) {
            ++index;
        }
        if (needle[index] == L'\0') {
            return (wchar_t *)start;
        }
    }
    return NULL;
}

wchar_t *wcspbrk(const wchar_t *text, const wchar_t *accept)
{
    for (const wchar_t *cursor = text; *cursor != L'\0'; ++cursor) {
        for (const wchar_t *candidate = accept; *candidate != L'\0'; ++candidate) {
            if (*cursor == *candidate) {
                return (wchar_t *)cursor;
            }
        }
    }
    return NULL;
}

size_t wcscspn(const wchar_t *text, const wchar_t *reject)
{
    size_t length = 0;
    while (text[length] != L'\0' && wcschr(reject, text[length]) == NULL) {
        ++length;
    }
    return length;
}

size_t wcsspn(const wchar_t *text, const wchar_t *accept)
{
    size_t length = 0;
    while (text[length] != L'\0' && wcschr(accept, text[length]) != NULL) {
        ++length;
    }
    return length;
}

size_t wcsxfrm(wchar_t *destination, const wchar_t *source, size_t count)
{
    const size_t length = wcslen(source);
    if (count > 0) {
        size_t copied = length < count - 1 ? length : count - 1;
        wmemcpy(destination, source, copied);
        destination[copied] = L'\0';
    }
    return length;
}

wchar_t *wmemset(wchar_t *destination, wchar_t character, size_t count)
{
    for (size_t index = 0; index < count; ++index) {
        destination[index] = character;
    }
    return destination;
}

wint_t btowc(int character)
{
    if (character == EOF || character < 0 || character > 0xff) {
        return WEOF;
    }
    return (wint_t)character;
}

int wctob(wint_t character)
{
    if (character == WEOF || character > 0xff) {
        return EOF;
    }
    return (int)character;
}

static void oct_mbstate_clear(mbstate_t *state)
{
    if (state == NULL) {
        return;
    }
    state->count = 0;
    state->pending = 0;
    state->bytes[0] = 0;
    state->bytes[1] = 0;
    state->bytes[2] = 0;
    state->bytes[3] = 0;
}

int mbsinit(const mbstate_t *state)
{
    if (state == NULL) {
        return 1;
    }
    return state->count == 0 && state->pending == 0;
}


size_t mbrlen(const char *input, size_t count, mbstate_t *state)
{
    return mbrtowc(NULL, input, count, state);
}


/* POSIX version of mbsrtowcs that also stops after `byte_count` input bytes. */
size_t mbsnrtowcs(wchar_t *destination, const char **source, size_t byte_count,
                  size_t count, mbstate_t *state)
{
    oct_mbstate_clear(state);
    const char *cursor = *source;
    size_t written = 0;
    size_t consumed = 0;
    while (consumed < byte_count && (destination == NULL || written < count)) {
        const unsigned char byte = (unsigned char)*cursor;
        if (destination != NULL) {
            destination[written] = (wchar_t)byte;
        }
        ++cursor;
        ++consumed;
        if (byte == 0) {
            *source = NULL;
            return written;
        }
        ++written;
    }
    *source = cursor;
    return written;
}

size_t mbsrtowcs(wchar_t *destination, const char **source, size_t count,
                 mbstate_t *state)
{
    oct_mbstate_clear(state);
    const char *cursor = *source;
    size_t written = 0;
    while (destination == NULL || written < count) {
        const unsigned char byte = (unsigned char)*cursor;
        if (destination != NULL) {
            destination[written] = (wchar_t)byte;
        }
        ++cursor;
        if (byte == 0) {
            *source = NULL;
            return written;
        }
        ++written;
    }
    *source = cursor;
    return written;
}

/* POSIX version of wcsrtombs that also stops after `wide_count` wide
 * characters, which is what libc++'s codecvt for wchar_t uses. */
size_t wcsnrtombs(char *destination, const wchar_t **source, size_t wide_count,
                  size_t count, mbstate_t *state)
{
    oct_mbstate_clear(state);
    const wchar_t *cursor = *source;
    size_t written = 0;
    size_t consumed = 0;
    while (consumed < wide_count && (destination == NULL || written < count)) {
        const wchar_t character = *cursor;
        if (character > 0xff) {
            return (size_t)-1;
        }
        if (destination != NULL) {
            destination[written] = (char)character;
        }
        ++cursor;
        ++consumed;
        if (character == 0) {
            *source = NULL;
            return written;
        }
        ++written;
    }
    *source = cursor;
    return written;
}

size_t wcsrtombs(char *destination, const wchar_t **source, size_t count,
                 mbstate_t *state)
{
    oct_mbstate_clear(state);
    const wchar_t *cursor = *source;
    size_t written = 0;
    while (destination == NULL || written < count) {
        const wchar_t character = *cursor;
        if (character > 0xff) {
            return (size_t)-1;
        }
        if (destination != NULL) {
            destination[written] = (char)character;
        }
        ++cursor;
        if (character == 0) {
            *source = NULL;
            return written;
        }
        ++written;
    }
    *source = cursor;
    return written;
}

/* Numeric conversions: copy the wide text into a narrow buffer and reuse the
 * narrow implementation, which already handles signs, bases and exponents. */
static size_t oct_wcs_to_narrow(char *buffer, size_t size, const wchar_t *text)
{
    size_t index = 0;
    while (index + 1 < size && text[index] != L'\0') {
        if (text[index] < 0 || text[index] > 0xff) {
            break;
        }
        buffer[index] = (char)text[index];
        ++index;
    }
    buffer[index] = '\0';
    return index;
}

double wcstod(const wchar_t *restrict text, wchar_t **restrict end)
{
    char buffer[128];
    oct_wcs_to_narrow(buffer, sizeof buffer, text);
    char *narrow_end = NULL;
    const double value = strtod(buffer, &narrow_end);
    if (end != NULL) {
        *end = (wchar_t *)text + (narrow_end != NULL ? (size_t)(narrow_end - buffer) : 0);
    }
    return value;
}

float wcstof(const wchar_t *restrict text, wchar_t **restrict end)
{
    return (float)wcstod(text, end);
}

long double wcstold(const wchar_t *restrict text, wchar_t **restrict end)
{
    return (long double)wcstod(text, end);
}


long long wcstoll(const wchar_t *restrict text, wchar_t **restrict end, int base)
{
    char buffer[128];
    oct_wcs_to_narrow(buffer, sizeof buffer, text);
    char *narrow_end = NULL;
    const long long value = strtoll(buffer, &narrow_end, base);
    if (end != NULL) {
        *end = (wchar_t *)text + (narrow_end != NULL ? (size_t)(narrow_end - buffer) : 0);
    }
    return value;
}

unsigned long wcstoul(const wchar_t *restrict text, wchar_t **restrict end, int base)
{
    char buffer[128];
    oct_wcs_to_narrow(buffer, sizeof buffer, text);
    char *narrow_end = NULL;
    const unsigned long value = strtoul(buffer, &narrow_end, base);
    if (end != NULL) {
        *end = (wchar_t *)text + (narrow_end != NULL ? (size_t)(narrow_end - buffer) : 0);
    }
    return value;
}

unsigned long long wcstoull(const wchar_t *restrict text, wchar_t **restrict end, int base)
{
    char buffer[128];
    oct_wcs_to_narrow(buffer, sizeof buffer, text);
    char *narrow_end = NULL;
    const unsigned long long value = strtoull(buffer, &narrow_end, base);
    if (end != NULL) {
        *end = (wchar_t *)text + (narrow_end != NULL ? (size_t)(narrow_end - buffer) : 0);
    }
    return value;
}

wchar_t *wmemcpy(wchar_t *destination, const wchar_t *source, size_t count)
{
    for (size_t index = 0; index < count; ++index) {
        destination[index] = source[index];
    }
    return destination;
}

wchar_t *wmemmove(wchar_t *destination, const wchar_t *source, size_t count)
{
    if (destination < source) {
        for (size_t index = 0; index < count; ++index) {
            destination[index] = source[index];
        }
    } else if (destination > source) {
        for (size_t index = count; index > 0; --index) {
            destination[index - 1] = source[index - 1];
        }
    }
    return destination;
}

int wmemcmp(const wchar_t *left, const wchar_t *right, size_t count)
{
    for (size_t index = 0; index < count; ++index) {
        if (left[index] != right[index]) {
            return left[index] < right[index] ? -1 : 1;
        }
    }
    return 0;
}

wchar_t *wmemchr(const wchar_t *memory, wchar_t character, size_t count)
{
    for (size_t index = 0; index < count; ++index) {
        if (memory[index] == character) {
            return (wchar_t *)(memory + index);
        }
    }
    return NULL;
}

/* Locale-aware wide conversions: the C locale is the only locale, so these
 * forward to the plain conversions. */
long wcstol_l(const wchar_t *text, wchar_t **end, int base, locale_t locale)
{
    (void)locale;
    return wcstol(text, end, base);
}

long long wcstoll_l(const wchar_t *text, wchar_t **end, int base, locale_t locale)
{
    (void)locale;
    return wcstoll(text, end, base);
}

unsigned long wcstoul_l(const wchar_t *text, wchar_t **end, int base, locale_t locale)
{
    (void)locale;
    return wcstoul(text, end, base);
}

unsigned long long wcstoull_l(const wchar_t *text, wchar_t **end, int base, locale_t locale)
{
    (void)locale;
    return wcstoull(text, end, base);
}

double wcstod_l(const wchar_t *text, wchar_t **end, locale_t locale)
{
    (void)locale;
    return wcstod(text, end);
}

float wcstof_l(const wchar_t *text, wchar_t **end, locale_t locale)
{
    (void)locale;
    return wcstof(text, end);
}

long double wcstold_l(const wchar_t *text, wchar_t **end, locale_t locale)
{
    (void)locale;
    return wcstold(text, end);
}

intmax_t wcstoimax(const wchar_t *text, wchar_t **end, int base)
{
    return (intmax_t)wcstoll(text, end, base);
}

uintmax_t wcstoumax(const wchar_t *text, wchar_t **end, int base)
{
    return (uintmax_t)wcstoull(text, end, base);
}

/*
 * Wide formatted output. Wide stdio streams (wprintf/fwprintf) still have no
 * implementation, but libc++'s <string> numeric helpers call swprintf for
 * std::to_wstring and the wide integer conversions, so the buffer form has to
 * exist. The locale is ASCII/Latin-1 and one wide character is one byte, so a
 * conversion is: format the narrow text, then widen it byte by byte.
 */

static size_t oct_swprintf_put(wchar_t *buffer, size_t count, size_t position,
                               wchar_t value)
{
    if (position + 1 < count) {
        buffer[position] = value;
    }
    return position + 1;
}

static size_t oct_swprintf_put_narrow(wchar_t *buffer, size_t count,
                                      size_t position, const char *text)
{
    while (*text != '\0') {
        position = oct_swprintf_put(buffer, count, position, (wchar_t)(unsigned char)*text);
        ++text;
    }
    return position;
}

static size_t oct_swprintf_put_wide(wchar_t *buffer, size_t count,
                                    size_t position, const wchar_t *text)
{
    while (*text != L'\0') {
        position = oct_swprintf_put(buffer, count, position, *text);
        ++text;
    }
    return position;
}

int vswprintf(wchar_t *buffer, size_t count, const wchar_t *format,
              va_list arguments)
{
    if (buffer == NULL || count == 0 || format == NULL) {
        return -1;
    }

    size_t written = 0;
    const wchar_t *cursor = format;

    while (*cursor != L'\0') {
        if (*cursor != L'%') {
            written = oct_swprintf_put(buffer, count, written, *cursor);
            ++cursor;
            continue;
        }

        ++cursor;
        if (*cursor == L'%') {
            written = oct_swprintf_put(buffer, count, written, L'%');
            ++cursor;
            continue;
        }

        /* Collect the specifier in narrow form; every C conversion specifier
         * is ASCII, so the copy is always safe. */
        char spec[40];
        size_t spec_length = 0;
        spec[spec_length++] = '%';

        int dynamic_width = 0;
        int dynamic_precision = 0;

        while (*cursor == L'-' || *cursor == L'+' || *cursor == L' ' ||
               *cursor == L'#' || *cursor == L'0' || *cursor == L'\'') {
            if (spec_length + 1 < sizeof(spec)) {
                spec[spec_length++] = (char)*cursor;
            }
            ++cursor;
        }

        if (*cursor == L'*') {
            dynamic_width = 1;
            ++cursor;
        } else {
            while (*cursor >= L'0' && *cursor <= L'9') {
                if (spec_length + 1 < sizeof(spec)) {
                    spec[spec_length++] = (char)*cursor;
                }
                ++cursor;
            }
        }

        if (*cursor == L'.') {
            if (spec_length + 1 < sizeof(spec)) {
                spec[spec_length++] = '.';
            }
            ++cursor;
            if (*cursor == L'*') {
                dynamic_precision = 1;
                ++cursor;
            } else {
                while (*cursor >= L'0' && *cursor <= L'9') {
                    if (spec_length + 1 < sizeof(spec)) {
                        spec[spec_length++] = (char)*cursor;
                    }
                    ++cursor;
                }
            }
        }

        /* OCTAGRAM only promises a narrow `int` for `*`, not the full C
         * negative-width rules; a negative width is treated as the absolute
         * value so the output stays well formed instead of corrupting the
         * specifier buffer. */
        if (dynamic_width) {
            int width = va_arg(arguments, int);
            if (width < 0) {
                width = -width;
            }
            char digits[16];
            size_t digit_count = 0;
            do {
                digits[digit_count++] = (char)('0' + (width % 10));
                width /= 10;
            } while (width > 0 && digit_count < sizeof(digits));
            while (digit_count > 0) {
                if (spec_length + 1 < sizeof(spec)) {
                    spec[spec_length++] = digits[--digit_count];
                } else {
                    --digit_count;
                }
            }
        }

        if (dynamic_precision) {
            int precision = va_arg(arguments, int);
            if (precision < 0) {
                precision = 0;
            }
            char digits[16];
            size_t digit_count = 0;
            do {
                digits[digit_count++] = (char)('0' + (precision % 10));
                precision /= 10;
            } while (precision > 0 && digit_count < sizeof(digits));
            while (digit_count > 0) {
                if (spec_length + 1 < sizeof(spec)) {
                    spec[spec_length++] = digits[--digit_count];
                } else {
                    --digit_count;
                }
            }
        }

        int longs = 0;
        while (*cursor == L'h' || *cursor == L'l' || *cursor == L'j' ||
               *cursor == L'z' || *cursor == L't' || *cursor == L'L') {
            if (*cursor == L'l') {
                ++longs;
            }
            if (spec_length + 1 < sizeof(spec)) {
                spec[spec_length++] = (char)*cursor;
            }
            ++cursor;
        }

        wchar_t conversion = *cursor;
        if (conversion == L'\0') {
            /* Truncated conversion: emit what was collected literally. */
            for (size_t index = 0; index < spec_length; ++index) {
                written = oct_swprintf_put(buffer, count, written, (wchar_t)spec[index]);
            }
            break;
        }
        ++cursor;

        if (spec_length + 1 < sizeof(spec)) {
            spec[spec_length++] = (char)conversion;
        }
        spec[spec_length] = '\0';

        switch (conversion) {
        case L's':
            if (longs > 0) {
                const wchar_t *text = va_arg(arguments, const wchar_t *);
                if (text == NULL) {
                    text = L"(null)";
                }
                written = oct_swprintf_put_wide(buffer, count, written, text);
            } else {
                const char *text = va_arg(arguments, const char *);
                if (text == NULL) {
                    text = "(null)";
                }
                written = oct_swprintf_put_narrow(buffer, count, written, text);
            }
            break;
        case L'c':
            if (longs > 0) {
                wint_t value = (wint_t)va_arg(arguments, wint_t);
                written = oct_swprintf_put(buffer, count, written, (wchar_t)value);
            } else {
                int value = va_arg(arguments, int);
                written = oct_swprintf_put(buffer, count, written, (wchar_t)(unsigned char)value);
            }
            break;
        case L'd':
        case L'i':
        case L'o':
        case L'u':
        case L'x':
        case L'X': {
            char narrowed[64];
            if (longs >= 2) {
                if (conversion == L'd' || conversion == L'i') {
                    snprintf(narrowed, sizeof(narrowed), spec, va_arg(arguments, long long));
                } else {
                    snprintf(narrowed, sizeof(narrowed), spec,
                             va_arg(arguments, unsigned long long));
                }
            } else if (longs == 1) {
                if (conversion == L'd' || conversion == L'i') {
                    snprintf(narrowed, sizeof(narrowed), spec, va_arg(arguments, long));
                } else {
                    snprintf(narrowed, sizeof(narrowed), spec,
                             va_arg(arguments, unsigned long));
                }
            } else {
                if (conversion == L'd' || conversion == L'i') {
                    snprintf(narrowed, sizeof(narrowed), spec, va_arg(arguments, int));
                } else {
                    snprintf(narrowed, sizeof(narrowed), spec,
                             va_arg(arguments, unsigned int));
                }
            }
            written = oct_swprintf_put_narrow(buffer, count, written, narrowed);
            break;
        }
        case L'f':
        case L'F':
        case L'e':
        case L'E':
        case L'g':
        case L'G':
        case L'a':
        case L'A': {
            char narrowed[128];
            if (longs > 0) {
                snprintf(narrowed, sizeof(narrowed), spec,
                         va_arg(arguments, long double));
            } else {
                snprintf(narrowed, sizeof(narrowed), spec, va_arg(arguments, double));
            }
            written = oct_swprintf_put_narrow(buffer, count, written, narrowed);
            break;
        }
        case L'p': {
            char narrowed[32];
            snprintf(narrowed, sizeof(narrowed), spec, va_arg(arguments, void *));
            written = oct_swprintf_put_narrow(buffer, count, written, narrowed);
            break;
        }
        case L'n': {
            /* %n is a security hazard and libc++ never needs it. */
            (void)va_arg(arguments, int *);
            break;
        }
        default: {
            char narrowed[2];
            narrowed[0] = (char)conversion;
            narrowed[1] = '\0';
            written = oct_swprintf_put_narrow(buffer, count, written, narrowed);
            break;
        }
        }
    }

    /* C99 swprintf always NUL-terminates when count > 0 and reports the
     * length the output would have had. */
    size_t terminator = written < count ? written : count - 1;
    buffer[terminator] = L'\0';
    return (int)written;
}

int swprintf(wchar_t *buffer, size_t count, const wchar_t *format, ...)
{
    va_list arguments;
    va_start(arguments, format);
    int result = vswprintf(buffer, count, format, arguments);
    va_end(arguments);
    return result;
}
