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

    // A pull on the currently checked-out branch updates refs/heads/<branch>,
    // not .git/HEAD (which still contains only "ref: refs/heads/<branch>").
    // Watch the refs directory and packed-refs too, so Cargo reruns this
    // script and refreshes the banner revision after git pull.
    for metadata_path in ["HEAD", "refs", "packed-refs"] {
        if let Some(path) = git_path(metadata_path) {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}

/// Resolve a path inside the active Git directory to an absolute path.
///
/// This also works with linked worktrees, where .git is a file and the
/// actual Git metadata lives elsewhere.
fn git_path(path: &str) -> Option<std::path::PathBuf> {
    let value = command_output(["rev-parse", "--git-path", path])?;
    let path = std::path::PathBuf::from(value);
    if path.is_absolute() {
        Some(path)
    } else {
        Some(std::env::current_dir().ok()?.join(path))
    }
}
fn command_output<const N: usize>(args: [&str; N]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
