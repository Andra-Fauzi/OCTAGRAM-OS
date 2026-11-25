/* Wide-character classification and case mapping (M4.3.2).
 *
 * Kept free of syscalls so it can be unit-tested on the host. The covered
 * repertoire is ASCII plus Latin-1, which matches the current single-locale
 * ctype implementation; full Unicode tables are a later step.
 */
#include <wctype.h>

enum {
    OCT_CLASS_ALNUM = 1u << 0,
    OCT_CLASS_ALPHA = 1u << 1,
    OCT_CLASS_BLANK = 1u << 2,
    OCT_CLASS_CNTRL = 1u << 3,
    OCT_CLASS_DIGIT = 1u << 4,
    OCT_CLASS_GRAPH = 1u << 5,
    OCT_CLASS_LOWER = 1u << 6,
    OCT_CLASS_PRINT = 1u << 7,
    OCT_CLASS_PUNCT = 1u << 8,
    OCT_CLASS_SPACE = 1u << 9,
    OCT_CLASS_UPPER = 1u << 10,
    OCT_CLASS_XDIGIT = 1u << 11,
};

static int is_latin1_upper(wint_t character)
{
    return character >= 0xc0 && character <= 0xde && character != 0xd7;
}

static int is_latin1_lower(wint_t character)
{
    return character >= 0xe0 && character <= 0xfe && character != 0xf7;
}

int iswdigit(wint_t character)
{
    return character >= '0' && character <= '9';
}

int iswxdigit(wint_t character)
{
    return iswdigit(character) || (character >= 'a' && character <= 'f') ||
           (character >= 'A' && character <= 'F');
}

int iswlower(wint_t character)
{
    return (character >= 'a' && character <= 'z') || is_latin1_lower(character);
}

int iswupper(wint_t character)
{
    return (character >= 'A' && character <= 'Z') || is_latin1_upper(character);
}

int iswalpha(wint_t character)
{
    return iswlower(character) || iswupper(character) ||
           (character >= 0xaa && character <= 0xb5) || character == 0xba ||
           (character >= 0xc0 && character <= 0xd6) ||
           (character >= 0xd8 && character <= 0xf6) ||
           (character >= 0xf8 && character <= 0xff);
}

int iswalnum(wint_t character)
{
    return iswalpha(character) || iswdigit(character);
}

int iswspace(wint_t character)
{
    return character == ' ' || (character >= '\t' && character <= '\r') ||
           (character >= 0x1c && character <= 0x1f) || character == 0xa0;
}

int iswblank(wint_t character)
{
    return character == ' ' || character == '\t';
}

int iswcntrl(wint_t character)
{
    return (character < 0x20) || (character >= 0x7f && character <= 0x9f);
}

int iswprint(wint_t character)
{
    return character >= 0x20 && character <= 0x7e;
}

int iswgraph(wint_t character)
{
    return iswprint(character) && character != ' ';
}

int iswpunct(wint_t character)
{
    return iswgraph(character) && !iswalnum(character);
}

wint_t towlower(wint_t character)
{
    if (character >= 'A' && character <= 'Z') {
        return character + ('a' - 'A');
    }
    if (is_latin1_upper(character)) {
        return character + 0x20;
    }
    return character;
}

wint_t towupper(wint_t character)
{
    if (character >= 'a' && character <= 'z') {
        return character - ('a' - 'A');
    }
    if (is_latin1_lower(character)) {
        return character - 0x20;
    }
    return character;
}

static int octagram_streq(const char *left, const char *right)
{
    while (*left != '\0' && *left == *right) {
        ++left;
        ++right;
    }
    return *left == *right;
}

wctype_t wctype(const char *name)
{
    if (name == NULL) {
        return 0;
    }
    if (octagram_streq(name, "alnum")) {
        return OCT_CLASS_ALNUM;
    }
    if (octagram_streq(name, "alpha")) {
        return OCT_CLASS_ALPHA;
    }
    if (octagram_streq(name, "blank")) {
        return OCT_CLASS_BLANK;
    }
    if (octagram_streq(name, "cntrl")) {
        return OCT_CLASS_CNTRL;
    }
    if (octagram_streq(name, "digit")) {
        return OCT_CLASS_DIGIT;
    }
    if (octagram_streq(name, "graph")) {
        return OCT_CLASS_GRAPH;
    }
    if (octagram_streq(name, "lower")) {
        return OCT_CLASS_LOWER;
    }
    if (octagram_streq(name, "print")) {
        return OCT_CLASS_PRINT;
    }
    if (octagram_streq(name, "punct")) {
        return OCT_CLASS_PUNCT;
    }
    if (octagram_streq(name, "space")) {
        return OCT_CLASS_SPACE;
    }
    if (octagram_streq(name, "upper")) {
        return OCT_CLASS_UPPER;
    }
    if (octagram_streq(name, "xdigit")) {
        return OCT_CLASS_XDIGIT;
    }
    return 0;
}

int iswctype(wint_t character, wctype_t description)
{
    switch (description) {
    case OCT_CLASS_ALNUM:
        return iswalnum(character);
    case OCT_CLASS_ALPHA:
        return iswalpha(character);
    case OCT_CLASS_BLANK:
        return iswblank(character);
    case OCT_CLASS_CNTRL:
        return iswcntrl(character);
    case OCT_CLASS_DIGIT:
        return iswdigit(character);
    case OCT_CLASS_GRAPH:
        return iswgraph(character);
    case OCT_CLASS_LOWER:
        return iswlower(character);
    case OCT_CLASS_PRINT:
        return iswprint(character);
    case OCT_CLASS_PUNCT:
        return iswpunct(character);
    case OCT_CLASS_SPACE:
        return iswspace(character);
    case OCT_CLASS_UPPER:
        return iswupper(character);
    case OCT_CLASS_XDIGIT:
        return iswxdigit(character);
    default:
        return 0;
    }
}

enum {
    OCT_TRANS_TOLOWER = 1,
    OCT_TRANS_TOUPPER = 2,
};

wctrans_t wctrans(const char *name)
{
    if (name == NULL) {
        return 0;
    }
    if (octagram_streq(name, "tolower")) {
        return OCT_TRANS_TOLOWER;
    }
    if (octagram_streq(name, "toupper")) {
        return OCT_TRANS_TOUPPER;
    }
    return 0;
}

wint_t towctrans(wint_t character, wctrans_t mapping)
{
    if (mapping == OCT_TRANS_TOLOWER) {
        return towlower(character);
    }
    if (mapping == OCT_TRANS_TOUPPER) {
        return towupper(character);
    }
    return character;
}

/*
 * POSIX locale-aware wide-character classes. Only the C locale exists, so the
 * locale argument is unused and every entry point forwards to its plain form.
 * libc++'s ctype<wchar_t>/codecvt facets call these through
 * _LIBCPP_GET_C_LOCALE in src/locale.cpp.
 */
#define OCT_DEFINE_WIDE_CLASS(name)                    \
    int name##_l(wint_t character, locale_t locale)    \
    {                                                  \
        (void)locale;                                  \
        return name(character);                        \
    }

OCT_DEFINE_WIDE_CLASS(iswalnum)
OCT_DEFINE_WIDE_CLASS(iswalpha)
OCT_DEFINE_WIDE_CLASS(iswblank)
OCT_DEFINE_WIDE_CLASS(iswcntrl)
OCT_DEFINE_WIDE_CLASS(iswdigit)
OCT_DEFINE_WIDE_CLASS(iswgraph)
OCT_DEFINE_WIDE_CLASS(iswlower)
OCT_DEFINE_WIDE_CLASS(iswprint)
OCT_DEFINE_WIDE_CLASS(iswpunct)
OCT_DEFINE_WIDE_CLASS(iswspace)
OCT_DEFINE_WIDE_CLASS(iswupper)
OCT_DEFINE_WIDE_CLASS(iswxdigit)

#undef OCT_DEFINE_WIDE_CLASS

int iswctype_l(wint_t character, wctype_t description, locale_t locale)
{
    (void)locale;
    return iswctype(character, description);
}

wint_t towlower_l(wint_t character, locale_t locale)
{
    (void)locale;
    return towlower(character);
}

wint_t towupper_l(wint_t character, locale_t locale)
{
    (void)locale;
    return towupper(character);
}
