#ifndef OCTAGRAM_CTYPE_H
#define OCTAGRAM_CTYPE_H

/* Only needed for the `locale_t` parameter of the *_l variants below. */
#include <locale.h>

static inline int isdigit(int character)
{
    return character >= '0' && character <= '9';
}

static inline int islower(int character)
{
    return character >= 'a' && character <= 'z';
}

static inline int isupper(int character)
{
    return character >= 'A' && character <= 'Z';
}

static inline int isalpha(int character)
{
    return islower(character) || isupper(character);
}

static inline int isalnum(int character)
{
    return isalpha(character) || isdigit(character);
}

static inline int iscntrl(int character)
{
    return (character >= 0 && character < 32) || character == 127;
}

static inline int isprint(int character)
{
    return character >= 32 && character <= 126;
}

static inline int isgraph(int character)
{
    return character > 32 && character <= 126;
}

static inline int isblank(int character)
{
    return character == ' ' || character == '\t';
}

/* POSIX extensions that libc++'s <locale> expects to find. */
static inline int isascii(int character)
{
    return (unsigned int)character <= 0x7f;
}

static inline int toascii(int character)
{
    return character & 0x7f;
}

static inline int ispunct(int character)
{
    return character >= '!' && character <= '~' && !isalnum(character);
}

static inline int isspace(int character)
{
    return character == ' ' || (character >= '\t' && character <= '\r');
}

static inline int isxdigit(int character)
{
    return isdigit(character) || (character >= 'a' && character <= 'f') ||
           (character >= 'A' && character <= 'F');
}

static inline int tolower(int character)
{
    return isupper(character) ? character - 'A' + 'a' : character;
}

static inline int toupper(int character)
{
    return islower(character) ? character - 'a' + 'A' : character;
}


/* POSIX.1-2008 locale-aware character classes. OCTAGRAM only has the C locale,
 * so every one of these ignores its locale argument and forwards. */
static inline int isalnum_l(int character, locale_t locale)
{
    (void)locale;
    return isalnum(character);
}

static inline int isalpha_l(int character, locale_t locale)
{
    (void)locale;
    return isalpha(character);
}

static inline int isblank_l(int character, locale_t locale)
{
    (void)locale;
    return isblank(character);
}

static inline int iscntrl_l(int character, locale_t locale)
{
    (void)locale;
    return iscntrl(character);
}

static inline int isdigit_l(int character, locale_t locale)
{
    (void)locale;
    return isdigit(character);
}

static inline int isgraph_l(int character, locale_t locale)
{
    (void)locale;
    return isgraph(character);
}

static inline int islower_l(int character, locale_t locale)
{
    (void)locale;
    return islower(character);
}

static inline int isprint_l(int character, locale_t locale)
{
    (void)locale;
    return isprint(character);
}

static inline int ispunct_l(int character, locale_t locale)
{
    (void)locale;
    return ispunct(character);
}

static inline int isspace_l(int character, locale_t locale)
{
    (void)locale;
    return isspace(character);
}

static inline int isupper_l(int character, locale_t locale)
{
    (void)locale;
    return isupper(character);
}

static inline int isxdigit_l(int character, locale_t locale)
{
    (void)locale;
    return isxdigit(character);
}

static inline int tolower_l(int character, locale_t locale)
{
    (void)locale;
    return tolower(character);
}

static inline int toupper_l(int character, locale_t locale)
{
    (void)locale;
    return toupper(character);
}

#endif
