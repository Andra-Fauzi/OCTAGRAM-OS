#!/usr/bin/env python3
"""Patch the installed rust-src component with the OCTAGRAM std PAL probe glue."""

from __future__ import annotations

import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def replace_once(path: Path, old: str, new: str) -> None:
    source = path.read_text(encoding="utf-8")
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"expected one exact Rust std source anchor in {path}, found {count}")
    path.write_text(source.replace(old, new, 1), encoding="utf-8")


def main() -> int:
    sysroot = Path(subprocess.check_output(
        ["rustc", "+nightly", "--print", "sysroot"], text=True
    ).strip())
    library = sysroot / "lib/rustlib/src/rust/library"
    std = library / "std/src"
    if not (std / "os/unix/mod.rs").is_file():
        raise SystemExit(f"rust-src component not found under {library}")

    os_mod = std / "os/mod.rs"
    replace_once(
        os_mod,
        "pub mod raw;\n",
        "pub mod raw;\n\n#[cfg(target_os = \"octagram\")]\npub(crate) mod octagram;\n",
    )

    unix_mod = std / "os/unix/mod.rs"
    replace_once(
        unix_mod,
        "    #[cfg(target_os = \"vxworks\")]\n"
        "    pub use crate::os::vxworks::*;\n",
        "    #[cfg(target_os = \"vxworks\")]\n"
        "    pub use crate::os::vxworks::*;\n"
        "    #[cfg(target_os = \"octagram\")]\n"
        "    pub(crate) use crate::os::octagram::*;\n",
    )
    thread_unix = std / "sys/thread/unix.rs"
    thread_anchor = (
        "impl Drop for Thread {\n"
        "    fn drop(&mut self) {\n"
        "        let ret = unsafe { libc::pthread_detach(self.id) };\n"
        "        debug_assert_eq!(ret, 0);\n"
        "    }\n"
        "}\n"
    )
    replace_once(
        thread_unix,
        thread_anchor,
        thread_anchor
        + "\n#[cfg(target_os = \"octagram\")]\n"
        + "pub fn set_name(_name: &CStr) {\n"
        + "    // Thread naming is not exposed by the current OCTAGRAM ABI.\n"
        + "}\n",
    )

    args_unix = std / "sys/args/unix.rs"
    replace_once(
        args_unix,
        '    target_os = "nuttx",\n))]\nmod imp {',
        '    target_os = "nuttx",\n    target_os = "octagram",\n))]\nmod imp {',
    )

    random_mod = std / "sys/random/mod.rs"
    replace_once(
        random_mod,
        '    any(target_os = "horizon", target_os = "cygwin", target_os = "redox") => {',
        '    target_os = "octagram" => {\n'
        '        mod getrandom;\n'
        '        pub use getrandom::fill_bytes;\n'
        '    }\n'
        '    any(target_os = "horizon", target_os = "cygwin", target_os = "redox") => {',
    )

    paths_unix = std / "sys/paths/unix.rs"
    current_exe_anchor = "#[cfg(target_os = \"aix\")]\npub fn current_exe() -> io::Result<PathBuf> {"
    replace_once(
        paths_unix,
        current_exe_anchor,
        "#[cfg(target_os = \"octagram\")]\n"
        + "pub fn current_exe() -> io::Result<PathBuf> {\n"
        + "    Err(io::Error::new(\n"
        + "        io::ErrorKind::Unsupported,\n"
        + "        \"current_exe is not supported by the current OCTAGRAM process ABI\",\n"
        + "    ))\n"
        + "}\n\n"
        + current_exe_anchor,
    )

    target = std / "os/octagram"
    target.mkdir(parents=True, exist_ok=True)
    for name in ("mod.rs", "raw.rs", "fs.rs"):
        destination = target / name
        if destination.exists():
            raise SystemExit(f"refusing to overwrite existing Rust std target file: {destination}")
        source = ROOT / "patches/rust-std/octagram" / name
        destination.write_text(source.read_text(encoding="utf-8"), encoding="utf-8")

    print("RUST-STD-OCTAGRAM-PATCH-PASS: Unix target glue installed in rust-src")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
