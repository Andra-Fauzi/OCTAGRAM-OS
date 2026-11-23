//! Runtime probe for the first upstream Rust std build on OCTAGRAM.
//!
//! This program is linked against upstream rust-src std and is embedded in the
//! kernel image so its process, filesystem, clock, thread and I/O paths can be
//! exercised under QEMU rather than merely type-checked on the host.

use std::io::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::process::{Command, Stdio};

fn marker(message: &str) {
    println!("{message}");
}

fn main() {
    // Exercise the upstream Unix FFI traits, not the OCTAGRAM helper wrappers.
    // Unix filenames are arbitrary bytes and must not be forced through UTF-8.
    let raw_name = vec![b'o', b'c', b't', 0xff];
    let os_name = std::ffi::OsString::from_vec(raw_name.clone());
    if os_name.as_bytes() != raw_name.as_slice() || os_name.clone().into_vec() != raw_name {
        marker("RUST-STD-OS-STRING-FFI-FAIL");
        std::process::exit(21);
    }
    marker("RUST-STD-OS-STRING-FFI-PASS");

    let path = std::path::PathBuf::from(os_name);
    if path.as_os_str().as_bytes() != [b'o', b'c', b't', 0xff] {
        marker("RUST-STD-PATH-OS-STRING-FFI-FAIL");
        std::process::exit(22);
    }
    marker("RUST-STD-PATH-OS-STRING-FFI-PASS");

    // Exercise upstream std::path lexical APIs with repeated and trailing
    // separators. Components must preserve "." and ".." rather than resolving
    // them through the filesystem.
    let lexical = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/octa//./gram/../file.bin/"));
    let components: Vec<_> = lexical.components().collect();
    if components.len() != 6
        || components[0] != std::path::Component::RootDir
        || components[1].as_os_str().as_bytes() != b"octa"
        || components[2] != std::path::Component::CurDir
        || components[3] != std::path::Component::Normal(std::ffi::OsStr::new("gram"))
        || components[4] != std::path::Component::ParentDir
        || components[5].as_os_str().as_bytes() != b"file.bin"
    {
        marker("RUST-STD-PATH-COMPONENTS-FAIL");
        std::process::exit(23);
    }
    if lexical.file_name().is_none_or(|name| name.as_bytes() != b"file.bin")
        || lexical.extension().is_none_or(|extension| extension.as_bytes() != b"bin")
    {
        marker("RUST-STD-PATH-FILENAME-FAIL");
        std::process::exit(24);
    }
    let mut joined = std::path::PathBuf::from("/octagram");
    joined.push("src");
    joined.push("main.rs");
    if joined != std::path::PathBuf::from("/octagram/src/main.rs") {
        marker("RUST-STD-PATH-JOIN-FAIL");
        std::process::exit(25);
    }
    marker("RUST-STD-PATH-LEXICAL-PASS");

    // Environment access must not panic when PATH is absent; the call itself
    // exercises std's environment PAL.
    let _path = std::env::var_os("PATH");
    marker("RUST-STD-ENV-PASS");

    let root = std::fs::metadata("/");
    if !root.as_ref().is_ok_and(|metadata| metadata.is_dir()) {
        marker("RUST-STD-FS-METADATA-FAIL");
        std::process::exit(11);
    }
    marker("RUST-STD-FS-METADATA-PASS");

    let entries = match std::fs::read_dir("/") {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("read_dir failed: {error}");
            std::process::exit(12);
        }
    };
    let mut count = 0usize;
    for entry in entries {
        if entry.is_err() {
            marker("RUST-STD-READ-DIR-FAIL");
            std::process::exit(13);
        }
        count += 1;
    }
    if count == 0 {
        marker("RUST-STD-READ-DIR-FAIL");
        std::process::exit(14);
    }
    marker("RUST-STD-READ-DIR-PASS");

    // Keep phase markers around the first upstream std thread. If the guest
    // faults before the closure marker, the failure is in pthread/TLS thread
    // initialization rather than the closure body or join result handling.
    let _main_thread = std::thread::current();
    marker("RUST-STD-THREAD-MAIN-CURRENT-PASS");
    let started = std::time::Instant::now();
    let handle = std::thread::spawn(|| {
        marker("RUST-STD-THREAD-CLOSURE-ENTER-PASS");
        7usize
    });
    marker("RUST-STD-THREAD-SPAWN-RETURN-PASS");
    handle
        .join()
        .ok()
        .filter(|value| *value == 7)
        .unwrap_or_else(|| {
            marker("RUST-STD-THREAD-FAIL");
            std::process::exit(15);
        });
    let _elapsed = started.elapsed();
    marker("RUST-STD-THREAD-CLOCK-PASS");

    // Separate the basic spawn/wait path from the stdio-pipe capture path.
    // If this passes but output() fails, the remaining fault is in pipe setup,
    // descriptor actions, or output collection rather than exec itself.
    let status = Command::new("/bin/sh")
        .arg("-c")
        .arg("exit 0")
        // Inherit stdin temporarily to isolate /dev/null handling from spawn/exec.
        .stdin(Stdio::inherit())
        .status()
        .unwrap_or_else(|error| {
            eprintln!("Command::status failed: {error}");
            std::process::exit(19);
        });
    if !status.success() {
        marker("RUST-STD-PROCESS-INHERITED-STDIO-FAIL");
        std::process::exit(20);
    }
    marker("RUST-STD-PROCESS-INHERITED-STDIO-PASS");

    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg("echo RUST-STD-CHILD-PASS")
        .stdin(Stdio::inherit())
        .output()
        .unwrap_or_else(|error| {
            eprintln!("Command::output failed: {error}");
            std::process::exit(16);
        });
    if !output.status.success() || !output.stdout.windows(19).any(|bytes| bytes == b"RUST-STD-CHILD-PASS") {
        marker("RUST-STD-PROCESS-FAIL");
        std::process::exit(17);
    }
    marker("RUST-STD-PROCESS-PASS");

    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(b"RUST-STD-IO-WRITE-PASS\n").is_err() || stdout.flush().is_err() {
        std::process::exit(18);
    }
    marker("RUST-STD-RUNTIME-PASS");
}
