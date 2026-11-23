//! Runtime probe for the first upstream Rust std build on OCTAGRAM.
//!
//! This program is linked against upstream rust-src std and is embedded in the
//! kernel image so its process, filesystem, clock, thread and I/O paths can be
//! exercised under QEMU rather than merely type-checked on the host.

use std::io::Write;
use std::process::Command;

fn marker(message: &str) {
    println!("{message}");
}

fn main() {
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
