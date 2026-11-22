#include <math.h>

extern long write(int, const void *, unsigned long);

int main(void)
{
    static const char pass[] = "TCC-MATH-PASS\n";

    if (fabs(-3.0) != 3.0 || sqrt(81.0) != 9.0)
        return 1;
    if (floor(-1.2) != -2.0 || ceil(-1.2) != -1.0)
        return 2;
    if (pow(2.0, 10.0) != 1024.0 || log2(8.0) != 3.0)
        return 3;
    if (fmod(5.5, 2.0) != 1.5 || remainder(7.0, 2.0) != -1.0)
        return 4;
    if (fabs(sin(0.52359877559829887308) - 0.5) > 1e-12)
        return 5;
    if (fabs(cos(1.04719755119659774615) - 0.5) > 1e-12)
        return 6;

    write(1, pass, sizeof(pass) - 1);
    return 0;
}
