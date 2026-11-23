//! Runtime probe for the first upstream Rust std build on OCTAGRAM.
//!
//! This program is linked against upstream rust-src std and is embedded in the
//! kernel image so its process, filesystem, clock, thread and I/O paths can be
//! exercised under QEMU rather than merely type-checked on the host.

use std::io::{Seek, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::process::{Command, Stdio};


// Minimal direct syscall helpers for testing the kernel's dirfd ABI independently
// of the upstream libc/std wrappers.
unsafe fn syscall3(number: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    let result: i64;
    unsafe {
        std::arch::asm!(
            "int 0x80",
            inlateout("rax") number as i64 => result,
            in("rdi") arg1,
            in("rsi") arg2,
            in("rdx") arg3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    result
}

unsafe fn syscall4(number: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64) -> i64 {
    let result: i64;
    unsafe {
        std::arch::asm!(
            "int 0x80",
            inlateout("rax") number as i64 => result,
            in("rdi") arg1,
            in("rsi") arg2,
            in("rdx") arg3,
            in("r10") arg4,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    result
}

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
    // separators. std::path normalizes interior "." and repeated separators,
    // while preserving ".." lexically rather than resolving it on disk.
    let lexical = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/octa//./gram/../file.bin/"));
    let components: Vec<_> = lexical.components().collect();
    if components.len() != 5
        || components[0] != std::path::Component::RootDir
        || components[1].as_os_str().as_bytes() != b"octa"
        || components[2] != std::path::Component::Normal(std::ffi::OsStr::new("gram"))
        || components[3] != std::path::Component::ParentDir
        || components[4].as_os_str().as_bytes() != b"file.bin"
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

    // Round-trip a real filename containing a non-UTF-8 byte through the
    // upstream std filesystem API. Keep the test isolated by including PID.
    let mut raw_file_name = b"octagram-rust-std-".to_vec();
    raw_file_name.extend_from_slice(std::process::id().to_string().as_bytes());
    raw_file_name.extend_from_slice(b"-");
    raw_file_name.push(0xff);
    let mut raw_file_path = std::path::PathBuf::from("/tmp");
    raw_file_path.push(std::ffi::OsString::from_vec(raw_file_name.clone()));
    let payload = b"octagram std filesystem byte-path probe";
    if let Err(error) = std::fs::write(&raw_file_path, payload) {
        eprintln!("non-UTF-8 path write failed: {error}");
        marker("RUST-STD-FS-BYTE-PATH-FAIL");
        std::process::exit(26);
    }
    let read_back = std::fs::read(&raw_file_path);
    let metadata = std::fs::metadata(&raw_file_path);
    let byte_name_found = std::fs::read_dir("/tmp")
        .is_ok_and(|entries| entries.filter_map(Result::ok).any(|entry| {
            entry.file_name().as_bytes() == raw_file_name.as_slice()
        }));
    let removed = std::fs::remove_file(&raw_file_path);
    if !matches!(read_back, Ok(ref bytes) if bytes.as_slice() == payload)
        || !metadata.as_ref().is_ok_and(|value| value.is_file() && value.len() == payload.len() as u64)
        || removed.is_err()
    {
        marker("RUST-STD-FS-BYTE-PATH-FAIL");
        std::process::exit(27);
    }
    marker("RUST-STD-FS-BYTE-PATH-PASS");
    if !byte_name_found {
        marker("RUST-STD-READ-DIR-BYTE-NAME-FAIL");
        std::process::exit(39);
    }
    marker("RUST-STD-READ-DIR-BYTE-NAME-PASS");

    // Exercise directory creation, file-descriptor I/O, seek, sync and rename
    // through upstream std. All paths are unique to this process.
    let base = format!("/tmp/octagram-rust-std-dir-{}", std::process::id());
    let original_dir = std::path::PathBuf::from(&base);
    let renamed_dir = std::path::PathBuf::from(format!("{base}-renamed"));
    let child_path = original_dir.join("probe.bin");
    let renamed_child = renamed_dir.join("probe.bin");
    if let Err(error) = std::fs::create_dir(&original_dir) {
        eprintln!("create_dir failed: {error}");
        marker("RUST-STD-FS-DIRECTORY-FAIL");
        std::process::exit(28);
    }
    let descriptor_result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&child_path)?;
        file.write_all(b"descriptor-roundtrip")?;
        let mut found_child = false;
        for entry in std::fs::read_dir(&original_dir)? {
            let entry = entry?;
            if entry.file_name() == std::ffi::OsStr::new("probe.bin")
                && entry.metadata()?.is_file()
            {
                found_child = true;
            }
        }
        if !found_child {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "nested read_dir entry missing"));
        }
        file.sync_all()?;
        let cloned = file.try_clone()?;
        if !file.metadata()?.is_file() || !cloned.metadata()?.is_file() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "descriptor metadata mismatch"));
        }
        file.seek(std::io::SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut bytes)?;
        if bytes != b"descriptor-roundtrip" {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "file contents mismatch"));
        }
        Ok(())
    })();
    if descriptor_result.is_err()
        || std::fs::rename(&original_dir, &renamed_dir).is_err()
        || !matches!(std::fs::read(&renamed_child), Ok(bytes) if bytes == b"descriptor-roundtrip")
    {
        let _ = std::fs::remove_file(&child_path);
        let _ = std::fs::remove_file(&renamed_child);
        let _ = std::fs::remove_dir(&original_dir);
        let _ = std::fs::remove_dir(&renamed_dir);
        marker("RUST-STD-FS-DIRECTORY-FAIL");
        std::process::exit(29);
    }
    if std::fs::remove_file(&renamed_child).is_err()
        || std::fs::remove_dir(&renamed_dir).is_err()
    {
        marker("RUST-STD-FS-DIRECTORY-CLEANUP-FAIL");
        std::process::exit(30);
    }
    marker("RUST-STD-FS-DIRECTORY-PASS");
    marker("RUST-STD-READ-DIR-NESTED-PASS");

    // Cargo and rustc commonly write a temporary artifact and atomically
    // replace the previous output with rename. Also exercise ftruncate.
    let truncate_path = std::path::PathBuf::from(format!(
        "/tmp/octagram-rust-truncate-{}.bin",
        std::process::id()
    ));
    let truncate_result = (|| -> std::io::Result<()> {
        std::fs::write(&truncate_path, b"truncate-this-payload")?;
        let file = std::fs::OpenOptions::new().write(true).open(&truncate_path)?;
        file.set_len(8)?;
        file.sync_data()?;
        if file.metadata()?.len() != 8 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "set_len mismatch"));
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(&truncate_path);
    if truncate_result.is_err() {
        marker("RUST-STD-FS-TRUNCATE-FAIL");
        std::process::exit(37);
    }
    marker("RUST-STD-FS-TRUNCATE-PASS");

    let atomic_base = format!("/tmp/octagram-rust-atomic-{}", std::process::id());
    let atomic_temp = std::path::PathBuf::from(format!("{atomic_base}.tmp"));
    let atomic_output = std::path::PathBuf::from(format!("{atomic_base}.out"));
    let atomic_result = (|| -> std::io::Result<()> {
        std::fs::write(&atomic_output, b"previous-artifact")?;
        std::fs::write(&atomic_temp, b"replacement-artifact")?;
        std::fs::File::open(&atomic_temp)?.sync_all()?;
        std::fs::rename(&atomic_temp, &atomic_output)?;
        let result = std::fs::read(&atomic_output)?;
        if result != b"replacement-artifact" {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "rename replacement mismatch"));
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(&atomic_temp);
    let _ = std::fs::remove_file(&atomic_output);
    if atomic_result.is_err() {
        marker("RUST-STD-FS-ATOMIC-RENAME-FAIL");
        std::process::exit(38);
    }
    marker("RUST-STD-FS-ATOMIC-RENAME-PASS");

    // Cargo frequently creates nested output/cache directories and resolves
    // paths relative to its working directory. Exercise those std APIs.
    let nested_dir = renamed_dir.join("target").join("debug").join("deps");
    if std::fs::create_dir_all(&nested_dir).is_err() {
        marker("RUST-STD-FS-CREATE-DIR-ALL-FAIL");
        std::process::exit(31);
    }
    let saved_cwd = match std::env::current_dir() {
        Ok(path) => path,
        Err(_) => {
            marker("RUST-STD-CURRENT-DIR-FAIL");
            std::process::exit(32);
        }
    };
    let canonical_nested = match std::fs::canonicalize(&nested_dir) {
        Ok(path) if path.is_absolute() => path,
        _ => {
            marker("RUST-STD-CANONICALIZE-FAIL");
            std::process::exit(33);
        }
    };
    if std::env::set_current_dir(&nested_dir).is_err()
        || !matches!(std::env::current_dir(), Ok(path) if path == canonical_nested)
        || std::env::set_current_dir(&saved_cwd).is_err()
    {
        let _ = std::env::set_current_dir(&saved_cwd);
        marker("RUST-STD-CURRENT-DIR-FAIL");
        std::process::exit(34);
    }
    if !std::fs::metadata(&canonical_nested).is_ok_and(|metadata| metadata.is_dir()) {
        marker("RUST-STD-CANONICALIZE-METADATA-FAIL");
        std::process::exit(35);
    }
    if std::fs::remove_dir_all(&renamed_dir).is_err() {
        marker("RUST-STD-REMOVE-DIR-ALL-FAIL");
        std::process::exit(36);
    }
    marker("RUST-STD-FS-PATH-MANAGEMENT-PASS");

    // Exercise the dirfd-relative POSIX ABI directly. These calls are kept
    // separate from std::fs path calls so a passing test proves that the
    // kernel resolves relative names against the supplied directory fd.
    let dirfd_file = match std::fs::File::open("/tmp") {
        Ok(file) => file,
        Err(error) => {
            eprintln!("open /tmp directory descriptor failed: {error}");
            marker("RUST-STD-AT-DIRFD-FAIL");
            std::process::exit(40);
        }
    };
    let dirfd = std::os::fd::AsRawFd::as_raw_fd(&dirfd_file) as u64;
    let stem = format!("octagram-at-{}", std::process::id());
    let directory_name = std::ffi::CString::new(format!("{stem}-dir")).unwrap();
    let source_name = std::ffi::CString::new(format!("{stem}-source")).unwrap();
    let target_name = std::ffi::CString::new(format!("{stem}-target")).unwrap();

    let mkdir_result = unsafe {
        syscall3(42, dirfd, directory_name.as_ptr() as usize as u64, 0o700)
    };
    if mkdir_result < 0 {
        marker("RUST-STD-AT-DIRFD-FAIL");
        std::process::exit(41);
    }
    let open_result = unsafe {
        syscall4(
            40,
            dirfd,
            source_name.as_ptr() as usize as u64,
            0x102, // O_CREAT | O_RDWR
            0o600,
        )
    };
    if open_result < 0 {
        let _ = unsafe { syscall3(43, dirfd, directory_name.as_ptr() as usize as u64, 0x200) };
        marker("RUST-STD-AT-DIRFD-FAIL");
        std::process::exit(42);
    }
    let mut relative_file = unsafe {
        <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(open_result as i32)
    };
    if relative_file.write_all(b"dirfd-relative-write").is_err()
        || relative_file.sync_all().is_err()
    {
        drop(relative_file);
        let _ = unsafe { syscall3(43, dirfd, source_name.as_ptr() as usize as u64, 0) };
        let _ = unsafe { syscall3(43, dirfd, directory_name.as_ptr() as usize as u64, 0x200) };
        marker("RUST-STD-AT-DIRFD-FAIL");
        std::process::exit(43);
    }
    drop(relative_file);
    let rename_result = unsafe {
        syscall4(
            44,
            dirfd,
            source_name.as_ptr() as usize as u64,
            dirfd,
            target_name.as_ptr() as usize as u64,
        )
    };
    let read_back = std::fs::read(std::path::Path::new("/tmp").join(
        std::ffi::OsStr::from_bytes(target_name.as_bytes())
    ));
    let unlink_file = unsafe {
        syscall3(43, dirfd, target_name.as_ptr() as usize as u64, 0)
    };
    let unlink_dir = unsafe {
        syscall3(43, dirfd, directory_name.as_ptr() as usize as u64, 0x200) // AT_REMOVEDIR
    };
    if rename_result < 0
        || !matches!(read_back, Ok(bytes) if bytes == b"dirfd-relative-write")
        || unlink_file < 0
        || unlink_dir < 0
    {
        marker("RUST-STD-AT-DIRFD-FAIL");
        std::process::exit(44);
    }
    marker("RUST-STD-AT-DIRFD-PASS");


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
