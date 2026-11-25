#ifndef OCTAGRAM_EXECINFO_H
#define OCTAGRAM_EXECINFO_H

/* glibc-compatible backtrace surface.
 *
 * OCTAGRAM resolves no symbol names at runtime (there is no ELF symbol index
 * for a static binary), so the *_symbols helpers produce one "0x..." line per
 * frame instead of glibc's "module(func+0x..)" text. `dladdr` on a dynamically
 * linked program does resolve names; use that when a name is required. */

#ifdef __cplusplus
extern "C" {
#endif

int backtrace(void **buffer, int size);
char **backtrace_symbols(void *const *buffer, int size);
void backtrace_symbols_fd(void *const *buffer, int size, int descriptor);

#ifdef __cplusplus
}
#endif
#endif
