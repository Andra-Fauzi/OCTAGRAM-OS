#ifndef OCTAGRAM_LIBGEN_H
#define OCTAGRAM_LIBGEN_H

#ifdef __cplusplus
extern "C" {
#endif

/* POSIX forms. Both may modify the buffer they are given:
 *   basename("/usr/lib/") -> "lib"
 *   dirname("/usr/lib")   -> "/usr"
 * <libgen.h> renames basename so it does not collide with the GNU form
 * declared by <string.h>, which never modifies its argument. */
char *dirname(char *path);
char *__xpg_basename(char *path);

#ifndef basename
#define basename __xpg_basename
#endif

#ifdef __cplusplus
}
#endif
#endif
