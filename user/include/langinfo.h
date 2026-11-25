/*
 * <langinfo.h> for OCTAGRAM.
 *
 * libc++'s src/locale.cpp includes <langinfo.h> unconditionally on Unix-like
 * targets and reads the C-locale strings from it (CODESET for the codecvt
 * facet, the day/month/time formats for std::time_put, RADIXCHAR/THOUSEP for
 * numpunct, YESEXPR/NOEXPR for messages, CRNCYSTR for moneypunct).
 *
 * OCTAGRAM only has the "C"/POSIX locale and it is ASCII/Latin-1, so
 * nl_langinfo() returns the same static C-locale strings for every item.
 *
 * The numbering is OCTAGRAM-local and sequential, not glibc's: glibc aliases
 * some items onto the same integer (for example ABMON_12 and ERA), which would
 * make a `switch` over nl_item ill-formed. Only the symbolic names are part of
 * the contract, and libc++ uses nothing else.
 */
#ifndef _OCTAGRAM_LANGINFO_H
#define _OCTAGRAM_LANGINFO_H

#ifdef __cplusplus
extern "C" {
#endif

typedef int nl_item;

enum {
    CODESET = 1,
    D_T_FMT,
    D_FMT,
    T_FMT,
    T_FMT_AMPM,
    AM_STR,
    PM_STR,

    DAY_1,
    DAY_2,
    DAY_3,
    DAY_4,
    DAY_5,
    DAY_6,
    DAY_7,
    ABDAY_1,
    ABDAY_2,
    ABDAY_3,
    ABDAY_4,
    ABDAY_5,
    ABDAY_6,
    ABDAY_7,

    MON_1,
    MON_2,
    MON_3,
    MON_4,
    MON_5,
    MON_6,
    MON_7,
    MON_8,
    MON_9,
    MON_10,
    MON_11,
    MON_12,
    ABMON_1,
    ABMON_2,
    ABMON_3,
    ABMON_4,
    ABMON_5,
    ABMON_6,
    ABMON_7,
    ABMON_8,
    ABMON_9,
    ABMON_10,
    ABMON_11,
    ABMON_12,

    RADIXCHAR,
    THOUSEP,
    CRNCYSTR,
    YESEXPR,
    NOEXPR,
    ERA,
    ERA_D_FMT,
    ERA_D_T_FMT,
    ERA_T_FMT,
    ALT_DIGITS
};

char *nl_langinfo(nl_item item);

#ifdef __cplusplus
}
#endif

#endif /* _OCTAGRAM_LANGINFO_H */
