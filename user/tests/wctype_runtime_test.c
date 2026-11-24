/* Host unit test for user/wctype.c. Runs on the development machine with the
 * same freestanding flags, so no OCTAGRAM syscalls are involved. */
#include <stdio.h>
#include <wctype.h>

static int failures;

static void check(int condition, const char *what)
{
    if (!condition) {
        printf("FAIL %s\n", what);
        ++failures;
    }
}

int main(void)
{
    check(iswalpha(L'A') && iswalpha(L'z'), "ASCII letters are alpha");
    check(!iswalpha(L'5') && !iswalpha(L' '), "digits and spaces are not alpha");
    check(iswdigit(L'7') && !iswdigit(L'a'), "digits");
    check(iswalnum(L'a') && iswalnum(L'9') && !iswalnum(L'-'), "alnum");
    check(iswspace(L' ') && iswspace(L'\t') && iswspace(L'\n'), "space");
    check(!iswspace(L'x'), "letters are not space");
    check(iswblank(L' ') && iswblank(L'\t') && !iswblank(L'\n'), "blank");
    check(iswcntrl(L'\0') && iswcntrl(L'\x7f') && !iswcntrl(L'a'), "cntrl");
    check(iswprint(L'!') && iswgraph(L'!') && !iswgraph(L' '), "print vs graph");
    check(iswpunct(L'!') && iswpunct(L',') && !iswpunct(L'a'), "punct");
    check(iswupper(L'A') && iswlower(L'a'), "upper and lower");
    check(iswxdigit(L'0') && iswxdigit(L'f') && iswxdigit(L'F') && !iswxdigit(L'g'),
          "xdigit");

    /* Latin-1 repertoire. */
    check(iswalpha(0xc5) && iswlower(0xe5), "Latin-1 letters");
    check(towlower(0xc5) == 0xe5 && towupper(0xe5) == 0xc5, "Latin-1 case mapping");

    check(towlower(L'Q') == L'q', "towlower ASCII");
    check(towupper(L'q') == L'Q', "towupper ASCII");
    check(towlower(L'5') == L'5' && towupper(L'#') == L'#', "mapping leaves others alone");

    check(wctype("alpha") != 0 && wctype("digit") != 0, "wctype resolves names");
    check(wctype("no-such-class") == 0, "unknown class name is rejected");
    check(iswctype(L'a', wctype("alpha")), "iswctype alpha");
    check(!iswctype(L'1', wctype("alpha")), "iswctype rejects wrong class");
    check(iswctype(L'1', wctype("digit")), "iswctype digit");

    check(wctrans("tolower") != 0 && wctrans("toupper") != 0, "wctrans resolves names");
    check(wctrans("bogus") == 0, "unknown mapping is rejected");
    check(towctrans(L'A', wctrans("tolower")) == L'a', "towctrans tolower");
    check(towctrans(L'a', wctrans("toupper")) == L'A', "towctrans toupper");
    check(towctrans(L'!', wctrans("toupper")) == L'!', "towctrans leaves punct alone");

    if (failures != 0) {
        printf("wctype runtime: FAIL (%d)\n", failures);
        return 1;
    }
    printf("wctype runtime: PASS\n");
    return 0;
}
