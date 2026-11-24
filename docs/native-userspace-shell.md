# Native userspace shell

OCTAGRAM now has an initial C userspace shell at `user/examples/sh.c`. The
program is built as `user/build/sh.elf` and embedded by `kernel/src/fs/mod.rs`
as `/bin/sh` in the initial VFS. The kernel's interactive shell remains
available; this executable is a separate POSIX-style command interpreter
intended to become the command processor used by GNU Make.

## Initial supported interface

- `/bin/sh -c 'command'` and an interactive stdin loop.
- External command lookup through `PATH`, using `fork`, `execvp`, and
  `waitpid`.
- Builtins: `cd`, `pwd`, `echo`, `printf`, `cat`, `export`, `unset`,
  `kill`, `exit`, `exec`, `true`, `false`, and `:`.
- `while COND; do BODY; done` (satu level) untuk proses latar dan skrip
  build sederhana.
- Command lists with `;`, `&&`, and `||`.
- Pipelines with `|`.
- Input redirection with `<`, output redirection with `>` and `>>`, and
  stderr redirection with `2>` / `2>&1`.
- Basic environment expansion using `$NAME`, `${NAME}`, `$?`, `$0`-`$9`,
  `$#`, dan `$$`. `sh -c 'command' name arg...` mengisi positional
  parameter sesuai POSIX.

## Deliberate limitations

This is a bring-up shell, not a complete POSIX shell. It does not yet implement
shell functions, general loops (only one-level `while ... do ... done` is
supported), conditionals, command substitution, subshells, glob expansion,
heredocs, job control, signal forwarding, or all POSIX quoting and expansion
rules. In particular, configure scripts and complex Make recipes may
still require a more complete shell such as a port of Dash. Do not mark GNU
Make recipe compatibility complete until the QEMU tests and real build recipes
pass.

## Guest validation

The QEMU syscall smoke test invokes `/bin/sh -c` and checks command execution,
a builtin pipeline, output truncation, and append redirection. These tests
validate the initial shell path only; they do not establish full POSIX shell
compatibility.
