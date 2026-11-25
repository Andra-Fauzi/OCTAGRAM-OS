// Second libc++ guest probe: the iostream/locale chain (std::cout,
// std::ostringstream, std::istringstream, manipulators, locale facets).
//
// This needs libc++'s locale facets (numpunct, ctype<wchar_t>, time_put,
// codecvt) behind sstream formatting, so it is a much wider test of the port
// than the string/charconv probe. Markers are printed with std::printf so a
// partial failure still names its stage in the serial log.

#include <cstdio>
#include <iomanip>
#include <iostream>
#include <locale>
#include <sstream>
#include <string>

int main() {
    std::ostringstream formatted;
    formatted << "value=" << 42 << ' ' << std::hex << 255 << ' ' << std::setw(4)
              << std::setfill('0') << 7;
    if (formatted.str() != "value=42 ff 0007") {
        std::printf("LIBCXX-IOSTREAM-FAIL: ostringstream\n");
        return 1;
    }
    std::printf("LIBCXX-OSTRINGSTREAM-PASS\n");

    std::istringstream input(" 12 34 word");
    int first = 0;
    int second = 0;
    std::string word;
    input >> first >> second >> word;
    if (input.fail() || first != 12 || second != 34 || word != "word") {
        std::printf("LIBCXX-IOSTREAM-FAIL: istringstream\n");
        return 1;
    }
    std::printf("LIBCXX-ISTRINGSTREAM-PASS\n");

    // Locale facets: numpunct/ctype/moneypunct come from the C locale strings
    // that nl_langinfo() and localeconv() provide.
    std::locale classic;
    const std::numpunct<char>& punctuation = std::use_facet<std::numpunct<char>>(classic);
    if (punctuation.decimal_point() != '.' || punctuation.thousands_sep() != '\0') {
        std::printf("LIBCXX-IOSTREAM-FAIL: numpunct\n");
        return 1;
    }
    const std::ctype<char>& characters = std::use_facet<std::ctype<char>>(classic);
    if (!characters.is(std::ctype_base::alpha, 'A') || characters.toupper('b') != 'B') {
        std::printf("LIBCXX-IOSTREAM-FAIL: ctype\n");
        return 1;
    }
    std::printf("LIBCXX-LOCALE-PASS\n");

    // Real stream output through libc++'s stdout buffer (fd 1).
    std::cout << "LIBCXX-COUT-LINE" << std::endl;
    if (!std::cout.good()) {
        std::printf("LIBCXX-IOSTREAM-FAIL: cout\n");
        return 1;
    }

    std::printf("LIBCXX-IOSTREAM-PASS\n");
    return 0;
}
