fn main() {
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("none") {
        println!("cargo:rustc-link-arg=-Tlinker-{arch}.ld");
        println!("cargo:rerun-if-changed=linker-{arch}.ld");
    }
    emit_build_stamp();
}

/// Stamp the kernel with its source revision and build time so the boot log can
/// name the exact binary that was flashed and a stale image is obvious at once.
fn emit_build_stamp() {
    let revision = command_output(["rev-parse", "--short=8", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = command_output(["status", "--porcelain", "--untracked-files=no", "--", "."])
        .map(|status| if status.is_empty() { "" } else { "+dirty" })
        .unwrap_or("");
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);

    println!("cargo:rustc-env=OCTAGRAM_BUILD_STAMP=rev={revision}{dirty} epoch={epoch}");
    // A new commit must produce a new stamp, even when no tracked source moved.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=build.rs");
}

fn command_output<const N: usize>(args: [&str; N]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
