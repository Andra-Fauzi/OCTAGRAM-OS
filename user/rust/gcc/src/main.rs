#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

mod fs;
#[path = "../../../../kernel/src/toolchain.rs"]
mod toolchain;

use alloc::ffi::CString;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::alloc::Layout;
use core::ffi::{c_char, CStr};
use core::panic::PanicInfo;
use core::slice;
use octagram_runtime::{
    chmod, close, exit, getcwd, open, read, write_all, O_CREAT, O_RDONLY, O_TRUNC, O_WRONLY,
    STDERR, STDOUT,
};

const MAX_FILE_BYTES: usize = 32 * 1024;

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8) -> i32 {
    let arguments = match collect_arguments(argc, argv) {
        Ok(arguments) => arguments,
        Err(error) => {
            report(&format!("gcc: {error}\n"));
            return 2;
        }
    };

    match compile_arguments(&arguments) {
        Ok(status) => status,
        Err(error) => {
            report(&format!("gcc: {error}\n"));
            1
        }
    }
}

fn collect_arguments(argc: i32, argv: *const *const u8) -> Result<Vec<String>, &'static str> {
    if argc < 1 || argv.is_null() {
        return Err("invalid argument vector");
    }
    let argc = usize::try_from(argc).map_err(|_| "invalid argument count")?;
    let pointers = unsafe { slice::from_raw_parts(argv, argc) };
    let mut arguments = Vec::new();
    arguments
        .try_reserve_exact(argc.saturating_sub(1))
        .map_err(|_| "out of memory")?;
    for pointer in pointers.iter().skip(1) {
        if pointer.is_null() {
            return Err("invalid argument");
        }
        let argument = unsafe { CStr::from_ptr(*pointer as *const c_char) };
        arguments.push(
            argument
                .to_str()
                .map_err(|_| "arguments must be UTF-8")?
                .to_string(),
        );
    }
    Ok(arguments)
}

fn compile_arguments(arguments: &[String]) -> Result<i32, String> {
    let mut input = None;
    let mut output = String::from("a.out");
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        match argument {
            "--help" => {
                write_all(
                    STDOUT,
                    b"usage: gcc <input.c> [-o output.elf] [-std=c11] [-O0|-O1|-O2]\n",
                )
                .map_err(|error| format!("write failed ({error})"))?;
                return Ok(0);
            }
            "--version" => {
                write_all(STDOUT, b"octagram gcc 0.1 (minimal C subset)\n")
                    .map_err(|error| format!("write failed ({error})"))?;
                return Ok(0);
            }
            "-o" => {
                index += 1;
                let Some(path) = arguments.get(index) else {
                    return Err(String::from("missing path after -o"));
                };
                output = path.clone();
            }
            "-std=c11"
            | "-O0"
            | "-O1"
            | "-O2"
            | "-Wall"
            | "-Wextra"
            | "-ffreestanding"
            | "-fno-pie"
            | "-fno-stack-protector" => {}
            option if option.starts_with('-') => {
                return Err(format!("unsupported option: {option}"));
            }
            path => {
                if input.replace(path.to_string()).is_some() {
                    return Err(String::from("only one C source file is supported"));
                }
            }
        }
        index += 1;
    }

    let Some(input) = input else {
        return Err(String::from("missing C source file"));
    };
    let cwd = current_directory()?;
    let source_path = if input.starts_with('/') {
        fs::normalize_path(&input)
    } else {
        fs::normalize_path(&format!("{cwd}/{input}"))
    }
    .map_err(|_| String::from("invalid source path"))?;
    let source =
        read_file(&source_path).map_err(|error| format!("cannot read {source_path}: {error:?}"))?;
    let executable = toolchain::compile_with_includes(&source, &source_path, read_file)
        .map_err(|error| format!("cannot compile {source_path}: {error:?}"))?;

    write_executable(&output, &executable)
        .map_err(|error| format!("cannot write {output}: {error}"))?;
    Ok(0)
}

fn current_directory() -> Result<String, String> {
    let mut buffer = [0u8; 256];
    let length = getcwd(&mut buffer).map_err(|error| format!("getcwd failed ({error})"))?;
    let path = CStr::from_bytes_with_nul(&buffer[..length])
        .map_err(|_| String::from("invalid current directory"))?
        .to_str()
        .map_err(|_| String::from("current directory is not UTF-8"))?;
    Ok(path.to_string())
}

fn read_file(path: &str) -> Result<Vec<u8>, fs::FsError> {
    let path = CString::new(path).map_err(|_| fs::FsError::InvalidPath)?;
    let descriptor = open(&path, O_RDONLY).map_err(map_fs_error)?;
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 1024];
    let read_result = loop {
        let count = match read(descriptor, &mut buffer) {
            Ok(count) => count,
            Err(error) => break Err(map_fs_error(error)),
        };
        if count == 0 {
            break Ok(());
        }
        let Some(new_length) = bytes.len().checked_add(count) else {
            break Err(fs::FsError::NoSpace);
        };
        if new_length > MAX_FILE_BYTES || bytes.try_reserve(count).is_err() {
            break Err(fs::FsError::NoSpace);
        }
        bytes.extend_from_slice(&buffer[..count]);
    };
    let close_result = close(descriptor);
    read_result?;
    close_result.map_err(map_fs_error)?;
    Ok(bytes)
}

fn write_executable(path: &str, bytes: &[u8]) -> Result<(), String> {
    let c_path = CString::new(path).map_err(|_| String::from("invalid output path"))?;
    let descriptor = open(&c_path, O_WRONLY | O_CREAT | O_TRUNC)
        .map_err(|error| format!("open failed ({error})"))?;
    let write_result = write_all(descriptor, bytes);
    let close_result = close(descriptor);
    write_result.map_err(|error| format!("write failed ({error})"))?;
    close_result.map_err(|error| format!("close failed ({error})"))?;
    chmod(&c_path, 0o755).map_err(|error| format!("chmod failed ({error})"))?;
    Ok(())
}

fn map_fs_error(error: i64) -> fs::FsError {
    match error {
        -2 => fs::FsError::NotFound,
        -13 => fs::FsError::PermissionDenied,
        -20 => fs::FsError::NotDirectory,
        _ => fs::FsError::IoError,
    }
}

fn report(message: &str) {
    let _ = write_all(STDERR, message.as_bytes());
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    report("gcc: compiler panicked\n");
    exit(101)
}

#[alloc_error_handler]
fn allocation_error(_layout: Layout) -> ! {
    report("gcc: out of memory\n");
    exit(127)
}
