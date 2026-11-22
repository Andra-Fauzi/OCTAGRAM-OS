//! Compile-only probe for the first upstream Rust std build on OCTAGRAM.
//! This binary is not linked or run yet; it exists to force std API type-checking.

fn main() {
    let _path = std::env::var_os("PATH");
    let _metadata = std::fs::metadata("/");
    let _clock = std::time::Instant::now();

    let child = std::thread::spawn(|| 7usize);
    let _joined = child.join();

    let _command = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("exit 0");

    let _directory = std::fs::read_dir("/");
}
