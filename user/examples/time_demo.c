#include <errno.h>
#include <stdio.h>
#include <time.h>

int main(void)
{
    struct timespec current;
    if (clock_gettime(CLOCK_REALTIME, &current) != 0 ||
        current.tv_sec <= 0 ||
        current.tv_nsec < 0 || current.tv_nsec >= 1000000000) {
        return 1;
    }

    time_t epoch = 951782400;
    struct tm *leap_day = gmtime(&epoch);
    if (leap_day == NULL || leap_day->tm_year != 100 ||
        leap_day->tm_mon != 1 || leap_day->tm_mday != 29 ||
        leap_day->tm_yday != 59 || leap_day->tm_wday != 2) {
        return 2;
    }

    time_t now = time(NULL);
    struct tm *local = localtime(&now);
    if (now < current.tv_sec - 1 || now > current.tv_sec + 1 ||
        local == NULL || local->tm_mon < 0 || local->tm_mon > 11 ||
        local->tm_mday < 1 || local->tm_mday > 31) {
        return 3;
    }
    puts("clock_gettime/time/localtime passed");
    return 0;
}
