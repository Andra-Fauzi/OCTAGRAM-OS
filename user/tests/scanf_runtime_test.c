#include <stdio.h>
#include <string.h>

int main(void)
{
    int signed_value = 0;
    int inferred_value = 0;
    unsigned int octal_value = 0;
    unsigned int unsigned_value = 0;
    char word[5] = {0};
    char character = 0;
    char digits[3] = {0};
    int consumed = -1;
    if (sscanf(" -42 0x2a 075 429 word Z 17!", "%d %i %o %u %4s %c %2[0-9]%n",
               &signed_value, &inferred_value, &octal_value, &unsigned_value,
               word, &character, digits, &consumed) != 7 ||
        signed_value != -42 || inferred_value != 42 || octal_value != 61 ||
        unsigned_value != 429 || strcmp(word, "word") != 0 ||
        character != 'Z' || strcmp(digits, "17") != 0 ||
        consumed <= 0) {
        return 1;
    }

    int suppressed_value = 0;
    if (sscanf("skip 123", "%*s %d", &suppressed_value) != 1 ||
        suppressed_value != 123) {
        return 2;
    }

    char set_value[4] = {0};
    if (sscanf("abc123!", "%3[a-z]", set_value) != 1 ||
        strcmp(set_value, "abc") != 0) {
        return 3;
    }
    if (sscanf("]rest", "%[]]", set_value) != 1 ||
        strcmp(set_value, "]") != 0) {
        return 4;
    }

    float float_value = 0.0f;
    double double_value = 0.0;
    long double long_double_value = 0.0L;
    int float_assignments = sscanf("1.25 0x1.8p+1 4.5e1", "%f %lf %Lf",
                                   &float_value, &double_value,
                                   &long_double_value);
    if (float_assignments != 3) {
        return 5;
    }
    if (float_value != 1.25f || double_value != 3.0 ||
        long_double_value != 45.0L) {
        return 5;
    }
    if (sscanf("x", "%lf", &double_value) != 0) {
        return 5;
    }
    float width_value = 0.0f;
    char width_remainder = '\0';
    if (sscanf("123.5x", "%4f%c", &width_value, &width_remainder) != 2 ||
        width_value != 123.0f || width_remainder != '5') {
        return 5;
    }

    int unchanged = 99;
    if (sscanf("not-a-number", "%d", &unchanged) != 0 || unchanged != 99 ||
        sscanf("0x", "%i", &unchanged) != 1 || unchanged != 0 ||
        sscanf("", "%d", &unchanged) != EOF) {
        return 6;
    }

    signed char small_signed = 0;
    unsigned short small_unsigned = 0;
    long long wide_signed = 0;
    size_t size_value = 0;
    if (sscanf("-7 65530 -123456789 1234", "%hhd %hu %lld %zu",
               &small_signed, &small_unsigned, &wide_signed, &size_value) != 4 ||
        small_signed != -7 || small_unsigned != 65530 ||
        wide_signed != -123456789 || size_value != 1234) {
        return 7;
    }

    char exact[3] = {0};
    if (sscanf("abc", "%3c", exact) != 1 ||
        exact[0] != 'a' || exact[1] != 'b' || exact[2] != 'c') {
        return 8;
    }
    return 0;
}
