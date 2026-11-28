#!/bin/sh
# Rust-only target linker adapter. It adds the freestanding executable's
# hidden __dso_handle object without changing the shared OCTAGRAM CRT/sysroot.
set -eu

driver=${OCTAGRAM_RUST_LINKER_DRIVER:-}
dso_object=${OCTAGRAM_RUST_DSO_OBJECT:-}

if [ -z "$driver" ] || [ ! -x "$driver" ]; then
    echo "octagram-rust-linker: OCTAGRAM_RUST_LINKER_DRIVER is missing or not executable" >&2
    exit 1
fi

# Compiler probes and compile-only requests must behave exactly like the
# normal driver. Inject the object only into an actual final link.
has_link_input=0
for argument in "$@"; do
    case $argument in
        *.o|*.a|*.rlib) has_link_input=1 ;;
    esac
done

if [ "$has_link_input" -eq 0 ]; then
    exec "$driver" "$@"
fi

if [ -z "$dso_object" ] || [ ! -s "$dso_object" ]; then
    echo "octagram-rust-linker: Rust __dso_handle object is missing" >&2
    echo "octagram-rust-linker: expected OCTAGRAM_RUST_DSO_OBJECT to name a non-empty object file" >&2
    exit 1
fi

exec "$driver" "$dso_object" "$@"
