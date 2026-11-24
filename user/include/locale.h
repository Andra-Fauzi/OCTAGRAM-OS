#ifndef OCTAGRAM_LOCALE_H
#define OCTAGRAM_LOCALE_H

#ifdef __cplusplus
extern "C" {
#endif

struct lconv {
    char *decimal_point;
    char *thousands_sep;
    char *grouping;
    char *int_curr_symbol;
    char *currency_symbol;
    char *mon_decimal_point;
    char *mon_thousands_sep;
    char *mon_grouping;
    char *positive_sign;
    char *negative_sign;
    char int_frac_digits;
    char frac_digits;
    char p_cs_precedes;
    char p_sep_by_space;
    char n_cs_precedes;
    char n_sep_by_space;
    char p_sign_posn;
    char n_sign_posn;
    char int_p_cs_precedes;
    char int_p_sep_by_space;
    char int_n_cs_precedes;
    char int_n_sep_by_space;
    char int_p_sign_posn;
    char int_n_sign_posn;
};

#define LC_CTYPE 0
#define LC_NUMERIC 1
#define LC_TIME 2
#define LC_COLLATE 3
#define LC_MONETARY 4
#define LC_MESSAGES 5
#define LC_ALL 6

/* POSIX.1-2008 category masks, used by the *_l and newlocale/duplocale family
 * and by libc++'s locale implementation. */
#define LC_CTYPE_MASK (1u << LC_CTYPE)
#define LC_NUMERIC_MASK (1u << LC_NUMERIC)
#define LC_TIME_MASK (1u << LC_TIME)
#define LC_COLLATE_MASK (1u << LC_COLLATE)
#define LC_MONETARY_MASK (1u << LC_MONETARY)
#define LC_MESSAGES_MASK (1u << LC_MESSAGES)
#define LC_ALL_MASK                                                              \
    (LC_CTYPE_MASK | LC_NUMERIC_MASK | LC_TIME_MASK | LC_COLLATE_MASK |          \
     LC_MONETARY_MASK | LC_MESSAGES_MASK)

/* POSIX.1-2008 locale objects. OCTAGRAM has exactly one locale (the C locale),
 * so every object is the same singleton and the *_l functions ignore it. */
typedef struct oct_ctagram_locale *locale_t;

#define LC_GLOBAL_LOCALE ((locale_t)-1L)

char *setlocale(int category, const char *locale);
struct lconv *localeconv(void);
locale_t newlocale(int category_mask, const char *name, locale_t base);
locale_t duplocale(locale_t object);
locale_t uselocale(locale_t object);
void freelocale(locale_t object);


#ifdef __cplusplus
}
#endif
#endif
