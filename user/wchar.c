/*
 * Wide-character string and conversion helpers required by the C99 <wchar.h>
 * API. libc++'s <cwchar> wrapper re-declares every one of these in namespace
 * std, so they have to exist for the C++ standard headers to compile.
 *
 * The OCTAGRAM locale is ASCII/Latin-1: one byte is one wide character, and no
 * shift state is ever needed. Wide stdio (fgetwc, wprintf, ...) is not
 * implemented yet; those entry points are declared but have no definition, so
 * using one fails at link time instead of silently misbehaving.
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
