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

    let started = std::time::Instant::now();
    std::thread::spawn(|| 7usize)
        .join()
        .ok()
        .filter(|value| *value == 7)
        .unwrap_or_else(|| {
            marker("RUST-STD-THREAD-FAIL");
            std::process::exit(15);
        });
    let _elapsed = started.elapsed();
    marker("RUST-STD-THREAD-CLOCK-PASS");

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
