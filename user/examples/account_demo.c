#include <errno.h>
#include <grp.h>
#include <pwd.h>
#include <stdio.h>
#include <string.h>

int main(void)
{
    struct passwd *user = getpwuid(0);
    if (user == NULL || strcmp(user->pw_name, "octagram") != 0) {
        return 1;
    }
    if (user->pw_uid != 0 || user->pw_gid != 0) {
        return 2;
    }
    if (strcmp(user->pw_dir, "/home/octagram") != 0 ||
        strcmp(user->pw_shell, "/bin/sh") != 0) {
        return 3;
    }

    struct passwd *by_name = getpwnam("octagram");
    if (by_name == NULL || by_name->pw_uid != 0 ||
        strcmp(by_name->pw_gecos, "OCTAGRAM User") != 0) {
        return 4;
    }
    if (getpwnam("missing") != NULL) {
        return 5;
    }

    struct passwd entry;
    struct passwd *result = NULL;
    char buffer[256];
    if (getpwnam_r("octagram", &entry, buffer, sizeof(buffer), &result) != 0 ||
        result != &entry || entry.pw_uid != 0 ||
        strcmp(entry.pw_name, "octagram") != 0) {
        return 6;
    }
    if (getpwuid_r(4242, &entry, buffer, sizeof(buffer), &result) != 0 ||
        result != NULL) {
        return 7;
    }
    char small_buffer[8];
    if (getpwnam_r("octagram", &entry, small_buffer, sizeof(small_buffer),
                   &result) != ERANGE) {
        return 8;
    }

    setpwent();
    struct passwd *iterated = getpwent();
    if (iterated == NULL || strcmp(iterated->pw_name, "octagram") != 0) {
        return 9;
    }
    endpwent();

    /* /etc/group belum tentu ada; runtime memakai grup utama dari passwd. */
    struct group *primary = getgrgid(0);
    if (primary == NULL || strcmp(primary->gr_name, "octagram") != 0 ||
        primary->gr_gid != 0 || primary->gr_mem[0] != NULL) {
        return 10;
    }
    struct group *group_by_name = getgrnam("octagram");
    if (group_by_name == NULL || group_by_name->gr_gid != 0) {
        return 11;
    }
    if (getgrnam("missing") != NULL) {
        return 12;
    }

    struct group group_entry;
    struct group *group_result = NULL;
    if (getgrgid_r(0, &group_entry, buffer, sizeof(buffer), &group_result) !=
            0 ||
        group_result != &group_entry ||
        strcmp(group_entry.gr_name, "octagram") != 0 ||
        group_entry.gr_mem[0] != NULL) {
        return 13;
    }
    if (getgrgid_r(4242, &group_entry, buffer, sizeof(buffer),
                   &group_result) != 0 ||
        group_result != NULL) {
        return 14;
    }

    return printf("pwd/grp database passed\n") < 0 ? 15 : 0;
}
