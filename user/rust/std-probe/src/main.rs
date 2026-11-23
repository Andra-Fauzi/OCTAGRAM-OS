//! Runtime probe for the first upstream Rust std build on OCTAGRAM.
//!
//! This program is linked against upstream rust-src std and is embedded in the
//! kernel image so its process, filesystem, clock, thread and I/O paths can be
//! exercised under QEMU rather than merely type-checked on the host.

use std::cell::Cell;
use std::io::{Seek, Write};

// Force a real non-empty PT_TLS image in the executable. This checks that
// Rust's local-exec TLS relocations and OCTAGRAM's startup TLS initializer
// agree on the target ABI instead of merely emitting an empty TLS header.
std::thread_local! {
    static RUST_TLS_PROBE: Cell<u64> = const { Cell::new(0x4f43_5441_4752_414d) };
}
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
    let (tls_value, tls_address) = RUST_TLS_PROBE.with(|value| {
        let initial = value.get();
        let address = value as *const Cell<u64> as usize;
        value.set(initial ^ 0x0101);
        ((initial, value.get()), address)
    });
    let fs_base: usize;
    unsafe {
        std::arch::asm!("mov {}, fs:0", out(reg) fs_base, options(nostack, preserves_flags));
    }
    if tls_value != (0x4f43_5441_4752_414d, 0x4f43_5441_4752_404c) {
        println!(
            "RUST-STD-TLS-DIAG initial={:#018x} changed={:#018x} address={:#x} fsbase={:#x}",
            tls_value.0, tls_value.1, tls_address, fs_base
        );
        marker("RUST-STD-TLS-FAIL");
        std::process::exit(68);
    }
    marker("RUST-STD-TLS-PASS");

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
    if let Err(error) = &descriptor_result {
        eprintln!("directory descriptor operation failed: {error}");
    }
    let directory_rename_result = std::fs::rename(&original_dir, &renamed_dir);
    if let Err(error) = &directory_rename_result {
        eprintln!("directory rename failed: {error}");
    }
    let renamed_read_result = std::fs::read(&renamed_child);
    if let Err(error) = &renamed_read_result {
        eprintln!("renamed child read failed: {error}");
    }
    if descriptor_result.is_err()
        || directory_rename_result.is_err()
        || !matches!(renamed_read_result, Ok(bytes) if bytes == b"descriptor-roundtrip")
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
    // Populate the deepest directory so remove_dir_all must enumerate and
    // unlink a child before removing each directory level (fdopendir/readdir).
    let recursive_file = nested_dir.join("cargo-probe.rmeta");
    if std::fs::write(&recursive_file, b"recursive directory cleanup").is_err()
        || !std::fs::metadata(&recursive_file).is_ok_and(|metadata| metadata.is_file())
    {
        marker("RUST-STD-REMOVE-DIR-ALL-SETUP-FAIL");
        std::process::exit(36);
    }
    if std::fs::remove_dir_all(&renamed_dir).is_err() {
        marker("RUST-STD-REMOVE-DIR-ALL-FAIL");
        std::process::exit(37);
    }
    if std::fs::symlink_metadata(&renamed_dir).is_ok() {
        marker("RUST-STD-REMOVE-DIR-ALL-LEFTOVER-FAIL");
        std::process::exit(38);
    }
    marker("RUST-STD-REMOVE-DIR-ALL-RECURSIVE-PASS");
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
    // Open the newly-created directory and use that descriptor for every
    // child operation. This proves nested dirfd resolution rather than merely
    // passing the same /tmp descriptor to each *at syscall.
    let nested_result = unsafe {
        syscall4(
            40,
            dirfd,
            directory_name.as_ptr() as usize as u64,
            0, // O_RDONLY; the path itself names a directory
            0,
        )
    };
    if nested_result < 0 {
        let _ = unsafe { syscall3(43, dirfd, directory_name.as_ptr() as usize as u64, 0x200) };
        marker("RUST-STD-AT-DIRFD-FAIL");
        std::process::exit(42);
    }
    let nested_directory = unsafe {
        <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(nested_result as i32)
    };
    let nested_dirfd = std::os::fd::AsRawFd::as_raw_fd(&nested_directory) as u64;
    let open_result = unsafe {
        syscall4(
            40,
            nested_dirfd,
            source_name.as_ptr() as usize as u64,
            0x102, // O_CREAT | O_RDWR
            0o600,
        )
    };
    if open_result < 0 {
        drop(nested_directory);
        let _ = unsafe { syscall3(43, dirfd, directory_name.as_ptr() as usize as u64, 0x200) };
        marker("RUST-STD-AT-DIRFD-FAIL");
        std::process::exit(43);
    }
    let mut relative_file = unsafe {
        <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(open_result as i32)
    };
    if relative_file.write_all(b"dirfd-relative-write").is_err()
        || relative_file.sync_all().is_err()
    {
        drop(relative_file);
        let _ = unsafe { syscall3(43, nested_dirfd, source_name.as_ptr() as usize as u64, 0) };
        drop(nested_directory);
        let _ = unsafe { syscall3(43, dirfd, directory_name.as_ptr() as usize as u64, 0x200) };
        marker("RUST-STD-AT-DIRFD-FAIL");
        std::process::exit(44);
    }
    drop(relative_file);
    let rename_result = unsafe {
        syscall4(
            44,
            nested_dirfd,
            source_name.as_ptr() as usize as u64,
            nested_dirfd,
            target_name.as_ptr() as usize as u64,
        )
    };
    let nested_path = std::path::Path::new("/tmp")
        .join(std::ffi::OsStr::from_bytes(directory_name.as_bytes()));
    let target_path = nested_path.join(std::ffi::OsStr::from_bytes(target_name.as_bytes()));
    let read_back = std::fs::read(&target_path);
    let unlink_file = unsafe {
        syscall3(43, nested_dirfd, target_name.as_ptr() as usize as u64, 0)
    };
    drop(nested_directory);
    let unlink_dir = unsafe {
        syscall3(43, dirfd, directory_name.as_ptr() as usize as u64, 0x200) // AT_REMOVEDIR
    };
    if rename_result < 0
        || !matches!(read_back, Ok(bytes) if bytes == b"dirfd-relative-write")
        || unlink_file < 0
        || unlink_dir < 0
    {
        marker("RUST-STD-AT-DIRFD-FAIL");
        std::process::exit(45);
    }
    marker("RUST-STD-AT-DIRFD-PASS");

    // Cargo and rustc use symlinks in source trees and build outputs. Exercise
    // the upstream std Unix symlink/read_link APIs, including lstat semantics.
    let symlink_base = format!("/tmp/octagram-rust-symlink-{}", std::process::id());
    let symlink_target = std::path::PathBuf::from(format!("{symlink_base}.target"));
    let symlink_path = std::path::PathBuf::from(format!("{symlink_base}.link"));
    let symlink_result = (|| -> std::io::Result<()> {
        std::fs::write(&symlink_target, b"symlink-target")?;
        std::os::unix::fs::symlink(&symlink_target, &symlink_path)?;
        let link_metadata = std::fs::symlink_metadata(&symlink_path)?;
        if !link_metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "symlink metadata mismatch"));
        }
        let link_destination = std::fs::read_link(&symlink_path)?;
        if link_destination != symlink_target {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "read_link target mismatch"));
        }
        if std::fs::read(&symlink_path)? != b"symlink-target" {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "symlink follow mismatch"));
        }
        std::fs::remove_file(&symlink_path)?;
        std::fs::remove_file(&symlink_target)?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&symlink_path);
    let _ = std::fs::remove_file(&symlink_target);
    if symlink_result.is_err() {
        marker("RUST-STD-FS-SYMLINK-FAIL");
        std::process::exit(46);
    }
    marker("RUST-STD-FS-SYMLINK-PASS");

    // Cargo copies build-script outputs and compiler artifacts between target
    // directories. Exercise the upstream std::fs::copy implementation and
    // verify both its byte count and the destination contents.
    let copy_source = std::path::PathBuf::from(format!(
        "/tmp/octagram-rust-copy-{}.source",
        std::process::id()
    ));
    let copy_destination = std::path::PathBuf::from(format!(
        "/tmp/octagram-rust-copy-{}.artifact",
        std::process::id()
    ));
    let copy_payload = b"cargo-artifact-copy-roundtrip";
    let copy_result = (|| -> std::io::Result<()> {
        std::fs::write(&copy_source, copy_payload)?;
        let copied = std::fs::copy(&copy_source, &copy_destination)?;
        let destination = std::fs::read(&copy_destination)?;
        if copied != copy_payload.len() as u64 || destination != copy_payload {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "copied artifact contents or byte count mismatch",
            ));
        }
        std::fs::remove_file(&copy_destination)?;
        std::fs::remove_file(&copy_source)?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&copy_destination);
    let _ = std::fs::remove_file(&copy_source);
    if copy_result.is_err() {
        marker("RUST-STD-FS-COPY-FAIL");
        std::process::exit(68);
    }
    marker("RUST-STD-FS-COPY-PASS");

    // Rust's filesystem API must create a second name for the same inode.
    // Removing the first name must not destroy the build artifact while the
    // second hard link still exists.
    let hard_link_source = std::path::PathBuf::from(format!(
        "/tmp/octagram-rust-hardlink-{}.source",
        std::process::id()
    ));
    let hard_link_alias = std::path::PathBuf::from(format!(
        "/tmp/octagram-rust-hardlink-{}.alias",
        std::process::id()
    ));
    let hard_link_payload = b"cargo-hard-link-artifact";
    let hard_link_result = (|| -> std::io::Result<()> {
        std::fs::write(&hard_link_source, hard_link_payload)?;
        std::fs::hard_link(&hard_link_source, &hard_link_alias)?;
        if std::fs::read(&hard_link_alias)? != hard_link_payload {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "hard-link contents mismatch",
            ));
        }
        std::fs::remove_file(&hard_link_source)?;
        if std::fs::read(&hard_link_alias)? != hard_link_payload {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "hard-link target disappeared with source name",
            ));
        }
        std::fs::remove_file(&hard_link_alias)?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&hard_link_alias);
    let _ = std::fs::remove_file(&hard_link_source);
    if hard_link_result.is_err() {
        marker("RUST-STD-FS-HARD-LINK-FAIL");
        std::process::exit(69);
    }
    marker("RUST-STD-FS-HARD-LINK-PASS");

    // Cargo build scripts and installed artifacts inspect and adjust Unix
    // permission bits. Exercise upstream std metadata and set_permissions.
    let permission_path = std::path::PathBuf::from(format!(
        "/tmp/octagram-rust-permissions-{}.bin",
        std::process::id()
    ));
    let permission_result = (|| -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&permission_path, b"permission-probe")?;
        let mut permissions = std::fs::metadata(&permission_path)?.permissions();
        permissions.set_mode(0o600);
        std::fs::set_permissions(&permission_path, permissions)?;
        let actual = std::fs::metadata(&permission_path)?.permissions().mode() & 0o777;
        if actual != 0o600 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "permission mode mismatch"));
        }
        std::fs::remove_file(&permission_path)?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&permission_path);
    if permission_result.is_err() {
        marker("RUST-STD-FS-PERMISSIONS-FAIL");
        std::process::exit(47);
    }
    marker("RUST-STD-FS-PERMISSIONS-PASS");

    // Environment access must not panic when PATH is absent; the call itself
    // exercises std's environment PAL.
    let _path = std::env::var_os("PATH");
    marker("RUST-STD-ENV-PASS");

    // Build scripts and tool launchers sometimes mutate their own environment
    // before spawning children. Check the upstream std set_var/remove_var path
    // against the process environment, including removal visibility.
    const MUTABLE_ENV: &str = "OCTAGRAM_STD_MUTABLE_ENV";
    unsafe {
        std::env::remove_var(MUTABLE_ENV);
        std::env::set_var(MUTABLE_ENV, "runtime-env-probe");
    }
    if !matches!(std::env::var(MUTABLE_ENV), Ok(value) if value == "runtime-env-probe") {
        marker("RUST-STD-ENV-MUTATION-FAIL");
        std::process::exit(50);
    }
    unsafe {
        std::env::remove_var(MUTABLE_ENV);
    }
    if std::env::var_os(MUTABLE_ENV).is_some() {
        marker("RUST-STD-ENV-MUTATION-FAIL");
        std::process::exit(51);
    }
    marker("RUST-STD-ENV-MUTATION-PASS");

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
        let tls_values = RUST_TLS_PROBE.with(|value| {
            let initial = value.get();
            value.set(initial ^ 0x0202);
            (initial, value.get())
        });
        (7usize, tls_values)
    });
    marker("RUST-STD-THREAD-SPAWN-RETURN-PASS");
    let worker_result = handle.join().unwrap_or_else(|_| {
        marker("RUST-STD-THREAD-FAIL");
        std::process::exit(15);
    });
    if worker_result != (
        7usize,
        (0x4f43_5441_4752_414d, 0x4f43_5441_4752_434f),
    ) {
        marker("RUST-STD-THREAD-TLS-ISOLATION-FAIL");
        std::process::exit(70);
    }
    let main_tls_after_join = RUST_TLS_PROBE.with(Cell::get);
    if main_tls_after_join != 0x4f43_5441_4752_404c {
        marker("RUST-STD-THREAD-TLS-ISOLATION-FAIL");
        std::process::exit(71);
    }
    marker("RUST-STD-THREAD-TLS-ISOLATION-PASS");
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

    // Verify argv forwarding, stderr capture, and non-zero child exit status.
    // Cargo build scripts and compiler helpers inspect all three channels.
    let process_io = Command::new("/bin/sh")
        .arg("-c")
        .arg("printf 'ARG:%s:%s\\n' \"$1\" \"$2\"; printf 'STDERR-PASS\\n' >&2; exit 7")
        .arg("probe-script")
        .arg("alpha")
        .arg("beta")
        .output()
        .unwrap_or_else(|error| {
            eprintln!("Command argument/stderr probe failed: {error}");
            std::process::exit(52);
        });
    if process_io.status.code() != Some(7)
        || !process_io.stdout.windows(b"ARG:alpha:beta\n".len())
            .any(|bytes| bytes == b"ARG:alpha:beta\n")
        || !process_io.stderr.windows(b"STDERR-PASS\n".len())
            .any(|bytes| bytes == b"STDERR-PASS\n")
    {
        marker("RUST-STD-PROCESS-ARGV-IO-FAIL");
        std::process::exit(53);
    }
    marker("RUST-STD-PROCESS-ARGV-IO-PASS");

    // A signal-terminated child must not be reported as an ordinary exit with
    // a synthetic 128+signal code. This checks kernel wait status encoding.
    use std::os::unix::process::ExitStatusExt;
    let signaled = Command::new("/bin/sh")
        .arg("-c")
        .arg("kill -TERM $$")
        .status()
        .unwrap_or_else(|error| {
            eprintln!("Command signal status probe failed: {error}");
            std::process::exit(54);
        });
    if signaled.code().is_some() || signaled.signal() != Some(15) {
        marker("RUST-STD-PROCESS-SIGNAL-STATUS-FAIL");
        std::process::exit(55);
    }
    marker("RUST-STD-PROCESS-SIGNAL-STATUS-PASS");

    // Child::try_wait must use waitpid(WNOHANG): it returns None while the
    // child is still running. Then Child::kill and Child::wait must reap it.
    let mut running_child = Command::new("/bin/sh")
        .arg("-c")
        .arg("while :; do :; done")
        .spawn()
        .unwrap_or_else(|error| {
            eprintln!("Command::spawn/try_wait probe failed: {error}");
            std::process::exit(56);
        });
    if running_child.id() == 0 {
        marker("RUST-STD-PROCESS-CHILD-ID-FAIL");
        std::process::exit(65);
    }
    marker("RUST-STD-PROCESS-CHILD-ID-PASS");
    match running_child.try_wait() {
        Ok(None) => {}
        Ok(Some(status)) => {
            marker("RUST-STD-PROCESS-TRY-WAIT-FAIL");
            eprintln!("child exited before try_wait returned: {status:?}");
            std::process::exit(57);
        }
        Err(error) => {
            let _ = running_child.kill();
            let _ = running_child.wait();
            marker("RUST-STD-PROCESS-TRY-WAIT-FAIL");
            eprintln!("Child::try_wait failed: {error}");
            std::process::exit(58);
        }
    }
    if let Err(error) = running_child.kill() {
        let _ = running_child.wait();
        marker("RUST-STD-PROCESS-KILL-FAIL");
        eprintln!("Child::kill failed: {error}");
        std::process::exit(59);
    }
    let killed_status = running_child.wait().unwrap_or_else(|error| {
        eprintln!("Child::wait after kill failed: {error}");
        std::process::exit(60);
    });
    if killed_status.code().is_some() || killed_status.signal() != Some(9) {
        marker("RUST-STD-PROCESS-KILL-FAIL");
        std::process::exit(61);
    }
    marker("RUST-STD-PROCESS-TRY-WAIT-KILL-PASS");

    // A successfully waited child must retain its cached status: repeated
    // wait/try_wait calls must not issue a second kernel reap.
    let repeated_wait = running_child.wait().unwrap_or_else(|error| {
        eprintln!("repeated Child::wait failed: {error}");
        std::process::exit(62);
    });
    let repeated_try_wait = running_child.try_wait().unwrap_or_else(|error| {
        eprintln!("Child::try_wait after wait failed: {error}");
        std::process::exit(63);
    });
    if repeated_wait.signal() != Some(9)
        || !repeated_try_wait.is_some_and(|status| status.signal() == Some(9))
    {
        marker("RUST-STD-PROCESS-WAIT-CACHE-FAIL");
        std::process::exit(64);
    }
    marker("RUST-STD-PROCESS-WAIT-CACHE-PASS");

    // Cargo build scripts rely on Command::env and current_dir being applied
    // to the child before exec. Verify both through the child's observable
    // environment and working directory, not merely by checking the builder.
    let process_context = Command::new("/bin/sh")
        .arg("-c")
        .arg("printf '%s\\n' \"$OCTAGRAM_STD_ENV\"; pwd")
        .env("OCTAGRAM_STD_ENV", "cargo-probe-env")
        .current_dir("/tmp")
        .output()
        .unwrap_or_else(|error| {
            eprintln!("Command environment/current_dir failed: {error}");
            std::process::exit(48);
        });
    let context_ok = process_context.status.success()
        && process_context.stdout.windows(b"cargo-probe-env\n".len())
            .any(|bytes| bytes == b"cargo-probe-env\n")
        && process_context.stdout.windows(b"/tmp\n".len())
            .any(|bytes| bytes == b"/tmp\n");
    if !context_ok {
        marker("RUST-STD-PROCESS-ENV-CWD-FAIL");
        std::process::exit(49);
    }
    marker("RUST-STD-PROCESS-ENV-CWD-PASS");

    // Cargo commonly launches helpers by name rather than by absolute path.
    // Verify that Command resolves a bare executable using the child PATH.
    let path_lookup = Command::new("sh")
        .arg("-c")
        .arg("printf 'RUST-STD-PATH-LOOKUP-CHILD\\n'")
        .env("PATH", "/bin")
        .output()
        .unwrap_or_else(|error| {
            eprintln!("Command PATH lookup failed: {error}");
            std::process::exit(66);
        });
    if !path_lookup.status.success()
        || !path_lookup.stdout.windows(b"RUST-STD-PATH-LOOKUP-CHILD\n".len())
            .any(|bytes| bytes == b"RUST-STD-PATH-LOOKUP-CHILD\n")
    {
        marker("RUST-STD-PROCESS-PATH-LOOKUP-FAIL");
        std::process::exit(67);
    }
    marker("RUST-STD-PROCESS-PATH-LOOKUP-PASS");

    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(b"RUST-STD-IO-WRITE-PASS\n").is_err() || stdout.flush().is_err() {
        std::process::exit(18);
    }
    marker("RUST-STD-RUNTIME-PASS");
}
