//! Small interactive shell for the framebuffer terminal and RAM VFS.

use alloc::string::String;
use alloc::vec::Vec;

use crate::drivers::keyboard;
use crate::fs::{FsError, Metadata, NodeType, Vfs};
use crate::storage::{block, fat32::Fat32};

const MAX_COMMAND_LENGTH: usize = 256;
const MAX_FILE_OPERATION: usize = 8192;
const MAX_TOOLCHAIN_FILE: usize = 72 * 1024;
const DEFAULT_PROGRAM_PATH: &[&str] = &["/bin", "/usr/bin", "/usr/local/bin"];

pub async fn run() {
    crate::println!("OCTAGRAM-OS shell");
    let mut current_dir = String::from("/");
    let mut program_path = DEFAULT_PROGRAM_PATH
        .iter()
        .map(|path| String::from(*path))
        .collect::<Vec<_>>();
    print_prompt(&current_dir);
    let mut line = String::new();

    loop {
        let keyboard::KeyEvent::Character(character) = keyboard::next_event().await else {
            continue;
        };
        match character {
            '\n' | '\r' => {
                crate::print!("\n");
                execute(&line, &mut current_dir, &mut program_path).await;
                line.clear();
                print_prompt(&current_dir);
            }
            '\u{8}' | '\u{7f}' => {
                if line.pop().is_some() {
                    crate::terminal::terminal_backspace();
                }
            }
            '\t' => {
                if line.len() < MAX_COMMAND_LENGTH {
                    line.push(' ');
                    crate::print!(" ");
                }
            }
            character if !character.is_control() && line.len() < MAX_COMMAND_LENGTH => {
                line.push(character);
                crate::print!("{character}");
            }
            _ => {}
        }
    }
}

fn print_prompt(current_dir: &str) {
    crate::print!("octagram:{current_dir} $ ");
}

async fn execute(line: &str, current_dir: &mut String, program_path: &mut Vec<String>) {
    let line = line.trim_start();
    let mut parts = line.splitn(2, char::is_whitespace);
    let command = parts.next().unwrap_or("");
    let arguments = parts.next().unwrap_or("").trim();
    if command.is_empty() {
        return;
    }

    match command {
        "help" => print_help(),
        "clear" => crate::terminal::terminal_clear(),
        "pwd" => crate::println!("{current_dir}"),
        "path" => print_program_path(program_path),
        "export" => set_program_path(current_dir, arguments, program_path),
        "cd" => change_directory(current_dir, arguments),
        "echo" => crate::println!("{arguments}"),
        "ls" => list_directory(current_dir, arguments),
        "cat" => show_file(current_dir, arguments),
        "touch" => create_node(current_dir, arguments, false),
        "mkdir" => create_node(current_dir, arguments, true),
        "ln" => create_symlink(current_dir, arguments),
        "rm" => remove_node(current_dir, arguments),
        "chmod" => change_permissions(current_dir, arguments),
        "stat" => show_metadata(current_dir, arguments),
        "cp" => copy_file(current_dir, arguments),
        "edit" => edit_file(current_dir, arguments).await,
        "devices" => list_block_devices(),
        "mount" => mount_device(current_dir, arguments),
        "umount" => unmount_device(current_dir, arguments),
        "as" => run_toolchain_command("as", current_dir, arguments),
        "ld" => run_toolchain_command("ld", current_dir, arguments),
        "cc" => run_toolchain_command("cc", current_dir, arguments),
        "run" => run_program(current_dir, arguments),
        "runp" => run_parallel_programs(current_dir, arguments),
        _ => run_external_program(current_dir, command, arguments, program_path),
    }
}

fn print_help() {
    crate::println!("Commands:");
    crate::println!("  help                   show this help");
    crate::println!("  clear                  clear the screen");
    crate::println!("  pwd                    show the current directory");
    crate::println!("  path                   show executable search directories");
    crate::println!("  export PATH=<dirs>     set colon-separated executable search paths");
    crate::println!("  cd [path]              change directory (default: /)");
    crate::println!("  ls [path]              list a directory");
    crate::println!("  cat <path>             print a UTF-8 file");
    crate::println!("  touch <path>           create an empty file");
    crate::println!("  mkdir <path>           create a directory");
    crate::println!("  ln -s <target> <link>  create a symbolic link");
    crate::println!("  rm <path>              remove an empty file or directory");
    crate::println!("  chmod <mode> <path>    set RAMFS permissions (e.g. +x, 755)");
    crate::println!("  stat <path>            show file metadata");
    crate::println!("  cp <source> <target>   copy a file");
    crate::println!("  edit <path>            edit a UTF-8 file");
    crate::println!("  devices                list detected storage devices");
    crate::println!("  mount <device> <path>  mount a FAT32 volume read-write");
    crate::println!("  umount <path>          unmount a filesystem");
    crate::println!("  as <input.s> -o <out.o> assemble supported x86_64 source");
    crate::println!("  ld <input.o> -o <out.elf> link a native toolchain object");
    crate::println!("  cc <input.c> -o <out.elf> compile the OCTAGRAM C subset");
    crate::println!("  echo <text>            print text");
    crate::println!("  run <path>             run a static x86_64 ELF");
    crate::println!("  runp <elf> <elf>...    run multiple ELFs with timer preemption");
    crate::println!("  <program>              run a program found in PATH");
}

fn change_directory(current_dir: &mut String, arguments: &str) {
    let path = if arguments.is_empty() {
        String::from("/")
    } else {
        let Some(path) = path_argument(current_dir, arguments) else {
            crate::println!("cd: invalid path");
            return;
        };
        path
    };

    match with_vfs(|vfs| vfs.metadata(&path)) {
        Ok(metadata) if metadata.node_type == NodeType::Directory => *current_dir = path,
        Ok(_) => crate::println!("cd: {path}: not a directory"),
        Err(error) => print_fs_error("cd", &path, error),
    }
}

fn list_directory(current_dir: &str, arguments: &str) {
    let path = match path_argument(current_dir, arguments) {
        Some(path) => path,
        None if arguments.is_empty() => String::from(current_dir),
        None => {
            crate::println!("ls: invalid path");
            return;
        }
    };
    match with_vfs(|vfs| vfs.read_dir(&path)) {
        Ok(entries) => {
            for entry in entries {
                let suffix = if entry.metadata.node_type == NodeType::Directory {
                    "/"
                } else if entry.metadata.node_type == NodeType::Symlink {
                    "@"
                } else {
                    ""
                };
                crate::println!("{}{}", entry.name, suffix);
            }
        }
        Err(error) => print_fs_error("ls", &path, error),
    }
}

fn show_file(current_dir: &str, arguments: &str) {
    let Some(path) = path_argument(current_dir, arguments) else {
        crate::println!("usage: cat <path>");
        return;
    };
    match with_vfs(|vfs| read_file(vfs, &path)) {
        Ok(bytes) => match core::str::from_utf8(&bytes) {
            Ok(text) => {
                crate::print!("{text}");
                if !text.is_empty() && !text.ends_with('\n') {
                    crate::print!("\n");
                }
            }
            Err(_) => crate::println!("cat: {path}: file is not valid UTF-8"),
        },
        Err(error) => print_fs_error("cat", &path, error),
    }
}

fn create_node(current_dir: &str, arguments: &str, directory: bool) {
    let command = if directory { "mkdir" } else { "touch" };
    let Some(path) = path_argument(current_dir, arguments) else {
        crate::println!("usage: {command} <path>");
        return;
    };
    let result = with_vfs(|vfs| {
        if directory {
            vfs.create_dir(&path)
        } else {
            vfs.create_file(&path)
        }
    });
    match result {
        Ok(()) => {}
        Err(error) => print_fs_error(command, &path, error),
    }
}

fn create_symlink(current_dir: &str, arguments: &str) {
    let mut parts = arguments.split_whitespace();
    let (Some("-s"), Some(target), Some(link), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        crate::println!("usage: ln -s <target> <link>");
        return;
    };
    let Some(link_path) = resolve_path(current_dir, link) else {
        crate::println!("ln: invalid link path");
        return;
    };
    match with_vfs(|vfs| vfs.symlink(target, &link_path)) {
        Ok(()) => {}
        Err(error) => crate::println!("ln: {link_path}: {error:?}"),
    }
}

fn remove_node(current_dir: &str, arguments: &str) {
    let Some(path) = path_argument(current_dir, arguments) else {
        crate::println!("usage: rm <path>");
        return;
    };
    match with_vfs(|vfs| vfs.remove(&path)) {
        Ok(()) => {}
        Err(error) => print_fs_error("rm", &path, error),
    }
}

fn change_permissions(current_dir: &str, arguments: &str) {
    let mut parts = arguments.split_whitespace();
    let (Some(mode), Some(path), None) = (parts.next(), parts.next(), parts.next()) else {
        crate::println!("usage: chmod <mode> <path>");
        return;
    };
    let Some(path) = resolve_path(current_dir, path) else {
        crate::println!("chmod: invalid path");
        return;
    };
    let result = with_vfs(|vfs| {
        let metadata = vfs.metadata(&path)?;
        let permissions = match mode {
            "+x" => metadata.permissions | 0o111,
            "-x" => metadata.permissions & !0o111,
            _ => u16::from_str_radix(mode, 8)
                .ok()
                .filter(|permissions| permissions & !0o777 == 0)
                .ok_or(FsError::InvalidData)?,
        };
        vfs.set_permissions(&path, permissions)
    });
    match result {
        Ok(()) => {}
        Err(error) => print_fs_error("chmod", &path, error),
    }
}

fn show_metadata(current_dir: &str, arguments: &str) {
    let Some(path) = path_argument(current_dir, arguments) else {
        crate::println!("usage: stat <path>");
        return;
    };
    match with_vfs(|vfs| vfs.metadata(&path)) {
        Ok(metadata) => print_metadata(&path, metadata),
        Err(error) => print_fs_error("stat", &path, error),
    }
}

fn copy_file(current_dir: &str, arguments: &str) {
    let mut paths = arguments.split_whitespace();
    let (Some(source), Some(target)) = (paths.next(), paths.next()) else {
        crate::println!("usage: cp <source> <target>");
        return;
    };
    if paths.next().is_some() {
        crate::println!("usage: cp <source> <target>");
        return;
    }
    let (Some(source), Some(target)) = (
        resolve_path(current_dir, source),
        resolve_path(current_dir, target),
    ) else {
        crate::println!("cp: invalid path");
        return;
    };

    match with_vfs(|vfs| {
        let data = read_file(vfs, &source)?;
        vfs.create_file(&target)?;
        if data.is_empty() {
            return Ok(());
        }
        match vfs.write(&target, 0, &data) {
            Ok(count) if count == data.len() => Ok(()),
            Ok(_) => {
                vfs.remove(&target)?;
                Err(FsError::NoSpace)
            }
            Err(error) => {
                vfs.remove(&target)?;
                Err(error)
            }
        }
    }) {
        Ok(()) => {}
        Err(error) => crate::println!("cp: {source} -> {target}: {error:?}"),
    }
}

fn run_program(current_dir: &str, arguments: &str) {
    let words = match split_shell_words(arguments) {
        Ok(words) if !words.is_empty() => words,
        _ => {
            crate::println!("usage: run <path> [arguments...]");
            return;
        }
    };
    let Some(path) = resolve_path(current_dir, &words[0]) else {
        crate::println!("usage: run <path>");
        return;
    };
    match crate::elf::run_file_from_with_args(&path, current_dir, &words) {
        Ok(status) => crate::println!("program exited with status {status}"),
        Err(error) => crate::println!("run: {path}: {error:?}"),
    }
}

fn run_parallel_programs(current_dir: &str, arguments: &str) {
    let words = match split_shell_words(arguments) {
        Ok(words) if words.len() >= 2 => words,
        _ => {
            crate::println!("usage: runp <elf> <elf> [elf...]");
            return;
        }
    };
    let mut paths = Vec::new();
    if paths.try_reserve_exact(words.len()).is_err() {
        crate::println!("runp: out of memory");
        return;
    }
    for word in &words {
        let Some(path) = resolve_path(current_dir, word) else {
            crate::println!("runp: invalid path: {word}");
            return;
        };
        paths.push(path);
    }
    match crate::elf::run_files_parallel_from(&paths, current_dir) {
        Ok(statuses) => {
            for (index, status) in statuses.iter().enumerate() {
                crate::println!("runp: pid {} exited with status {status}", index + 1);
            }
        }
        Err(error) => crate::println!("runp: {error:?}"),
    }
}

fn print_program_path(program_path: &[String]) {
    crate::println!("{}", program_path.join(":"));
}

fn set_program_path(current_dir: &str, arguments: &str, program_path: &mut Vec<String>) {
    let Some(value) = arguments.strip_prefix("PATH=") else {
        crate::println!("usage: export PATH=<dir>:<dir>:...");
        return;
    };
    let mut paths = Vec::new();
    for path in value.split(':').filter(|path| !path.is_empty()) {
        let Some(path) = resolve_path(current_dir, path) else {
            crate::println!("export: invalid PATH entry");
            return;
        };
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    *program_path = paths;
}

fn run_external_program(
    current_dir: &str,
    command: &str,
    arguments: &str,
    program_path: &[String],
) {
    let extra_arguments = match split_shell_words(arguments) {
        Ok(arguments) => arguments,
        Err(()) => {
            crate::println!("{command}: unmatched quote or escape");
            return;
        }
    };
    let Some(candidates) = program_candidates(current_dir, command, program_path) else {
        crate::println!("{command}: invalid path");
        return;
    };
    let mut argv = Vec::with_capacity(extra_arguments.len() + 1);
    argv.push(String::from(command));
    argv.extend(extra_arguments);
    let mut permission_denied = None;
    for path in candidates {
        match with_vfs(|vfs| vfs.metadata(&path)) {
            Ok(metadata)
                if metadata.node_type == NodeType::File && metadata.permissions & 0o111 != 0 =>
            {
                match crate::elf::run_file_from_with_args(&path, current_dir, &argv) {
                    Ok(status) => crate::println!("program exited with status {status}"),
                    Err(error) => crate::println!("{command}: {error:?}"),
                }
                return;
            }
            Ok(metadata) if metadata.node_type == NodeType::File => {
                permission_denied = Some(path);
            }
            Ok(_) => {
                crate::println!("{command}: {path}: not an executable file");
                return;
            }
            Err(FsError::NotFound) => {}
            Err(error) => {
                print_fs_error(command, &path, error);
                return;
            }
        }
    }
    if let Some(path) = permission_denied {
        crate::println!("{command}: {path}: permission denied");
        return;
    }
    crate::println!("{command}: command not found");
}

fn split_shell_words(source: &str) -> Result<Vec<String>, ()> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut word_started = false;

    for character in source.chars() {
        if escaped {
            word.push(character);
            escaped = false;
            word_started = true;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            word_started = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            } else {
                word.push(character);
            }
            continue;
        }
        match character {
            '\'' | '"' => {
                quote = Some(character);
                word_started = true;
            }
            character if character.is_whitespace() => {
                if word_started {
                    words.push(core::mem::take(&mut word));
                    word_started = false;
                }
            }
            _ => {
                word.push(character);
                word_started = true;
            }
        }
    }

    if escaped || quote.is_some() {
        return Err(());
    }
    if word_started {
        words.push(word);
    }
    Ok(words)
}

fn program_candidates(
    current_dir: &str,
    command: &str,
    program_path: &[String],
) -> Option<Vec<String>> {
    if command.is_empty() {
        return None;
    }
    if command.contains('/') {
        return Some(alloc::vec![resolve_path(current_dir, command)?]);
    }
    Some(
        program_path
            .iter()
            .filter_map(|directory| resolve_path(directory, command))
            .collect(),
    )
}

fn list_block_devices() {
    let devices = block::list();
    if devices.is_empty() {
        crate::println!("no storage devices detected");
        return;
    }
    for device in devices {
        crate::println!(
            "{}  {} sectors ({} MiB)",
            device.name,
            device.sectors,
            device.sectors.saturating_mul(512) / (1024 * 1024)
        );
    }
}

fn mount_device(current_dir: &str, arguments: &str) {
    let mut parts = arguments.split_whitespace();
    let (Some(device_name), Some(mountpoint), None) = (parts.next(), parts.next(), parts.next())
    else {
        crate::println!("usage: mount <device> <path>");
        return;
    };
    let Some(mountpoint) = resolve_path(current_dir, mountpoint) else {
        crate::println!("mount: invalid mount path");
        return;
    };
    let Some(device) = block::get(device_name) else {
        crate::println!("mount: storage device `{device_name}` not found (try `devices`)");
        return;
    };
    match Fat32::mount(device) {
        Ok(filesystem) => match with_vfs(|vfs| vfs.mount(&mountpoint, filesystem)) {
            Ok(()) => crate::println!("mounted {device_name} at {mountpoint} (FAT32, read-write)"),
            Err(error) => crate::println!("mount: {mountpoint}: {error:?}"),
        },
        Err(error) => crate::println!("mount: {device_name}: {error:?}"),
    }
}

fn unmount_device(current_dir: &str, arguments: &str) {
    let mut parts = arguments.split_whitespace();
    let (Some(path), None) = (parts.next(), parts.next()) else {
        crate::println!("usage: umount <path>");
        return;
    };
    let Some(path) = resolve_path(current_dir, path) else {
        crate::println!("umount: invalid path");
        return;
    };
    match with_vfs(|vfs| vfs.unmount(&path)) {
        Ok(()) => crate::println!("unmounted {path}"),
        Err(error) => crate::println!("umount: {path}: {error:?}"),
    }
}

fn run_toolchain_command(command: &str, current_dir: &str, arguments: &str) {
    let mut parts = arguments.split_whitespace();
    let (Some(input), Some("-o"), Some(output), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        crate::println!("usage: {command} <input> -o <output>");
        return;
    };
    let (Some(input), Some(output)) = (
        resolve_path(current_dir, input),
        resolve_path(current_dir, output),
    ) else {
        crate::println!("{command}: invalid path");
        return;
    };
    let source = match read_toolchain_file(&input) {
        Ok(bytes) => bytes,
        Err(error) => {
            print_fs_error(command, &input, error);
            return;
        }
    };
    let result = match command {
        "as" => crate::toolchain::assemble(&source),
        "ld" => crate::toolchain::link(&source),
        "cc" => crate::toolchain::compile_with_includes(&source, &input, read_toolchain_file),
        _ => return,
    };
    let output_bytes = match result {
        Ok(bytes) => bytes,
        Err(error) => {
            crate::println!("{command}: {error:?}");
            return;
        }
    };
    match write_toolchain_file(&output, &output_bytes, matches!(command, "ld" | "cc")) {
        Ok(()) => crate::println!("{command}: wrote {} bytes to {output}", output_bytes.len()),
        Err(error) => print_fs_error(command, &output, error),
    }
}

fn read_toolchain_file(path: &str) -> Result<Vec<u8>, FsError> {
    with_vfs(|vfs| {
        let metadata = vfs.metadata(path)?;
        if metadata.node_type != NodeType::File {
            return Err(FsError::IsDirectory);
        }
        if metadata.len > MAX_TOOLCHAIN_FILE {
            return Err(FsError::NoSpace);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(metadata.len)
            .map_err(|_| FsError::NoSpace)?;
        bytes.resize(metadata.len, 0);
        if vfs.read(path, 0, &mut bytes)? != metadata.len {
            return Err(FsError::IoError);
        }
        Ok(bytes)
    })
}

fn write_toolchain_file(path: &str, bytes: &[u8], executable: bool) -> Result<(), FsError> {
    with_vfs(|vfs| {
        match vfs.metadata(path) {
            Ok(metadata) if metadata.node_type == NodeType::File => vfs.truncate(path, 0)?,
            Ok(_) => return Err(FsError::IsDirectory),
            Err(FsError::NotFound) => vfs.create_file(path)?,
            Err(error) => return Err(error),
        }
        if !bytes.is_empty() && vfs.write(path, 0, bytes)? != bytes.len() {
            return Err(FsError::NoSpace);
        }
        vfs.truncate(path, bytes.len())?;
        if executable {
            let metadata = vfs.metadata(path)?;
            if metadata.permissions & 0o111 == 0 {
                vfs.set_permissions(path, metadata.permissions | 0o111)?;
            }
        }
        Ok(())
    })
}

async fn edit_file(current_dir: &str, arguments: &str) {
    let Some(path) = path_argument(current_dir, arguments) else {
        crate::println!("usage: edit <path>");
        return;
    };
    let bytes = match open_or_create_editor_file(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            print_fs_error("edit", &path, error);
            return;
        }
    };
    let mut text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => {
            crate::println!("edit: {path}: file is not valid UTF-8");
            return;
        }
    };
    let mut cursor = text.len();
    let mut dirty = false;
    let mut confirming_discard = false;
    let mut status = String::new();
    render_editor(&path, &text, cursor, &status);

    loop {
        match keyboard::next_event().await {
            keyboard::KeyEvent::Character('\x13') => {
                match save_editor_file(&path, &text) {
                    Ok(()) => {
                        dirty = false;
                        status = String::from("Saved");
                    }
                    Err(error) => {
                        status = alloc::format!("Save failed: {error:?}");
                    }
                }
                confirming_discard = false;
            }
            keyboard::KeyEvent::Character('\x11') => {
                if dirty && !confirming_discard {
                    status = String::from("Unsaved changes: Ctrl+Q again to discard");
                    confirming_discard = true;
                } else {
                    crate::terminal::terminal_clear();
                    return;
                }
            }
            keyboard::KeyEvent::Character('\x08' | '\x7f') => {
                if let Some(previous) = previous_char_boundary(&text, cursor) {
                    text.remove(previous);
                    cursor = previous;
                    dirty = true;
                }
                status.clear();
                confirming_discard = false;
            }
            keyboard::KeyEvent::Character('\n' | '\r') => {
                if text.len() < MAX_FILE_OPERATION && text.try_reserve(1).is_ok() {
                    text.insert(cursor, '\n');
                    cursor += 1;
                    dirty = true;
                }
                status.clear();
                confirming_discard = false;
            }
            keyboard::KeyEvent::Character(character) if !character.is_control() => {
                if text.len() + character.len_utf8() <= MAX_FILE_OPERATION
                    && text.try_reserve(character.len_utf8()).is_ok()
                {
                    text.insert(cursor, character);
                    cursor += character.len_utf8();
                    dirty = true;
                }
                status.clear();
                confirming_discard = false;
            }
            keyboard::KeyEvent::Left => {
                if let Some(previous) = previous_char_boundary(&text, cursor) {
                    cursor = previous;
                }
                status.clear();
                confirming_discard = false;
            }
            keyboard::KeyEvent::Right => {
                cursor = next_char_boundary(&text, cursor);
                status.clear();
                confirming_discard = false;
            }
            keyboard::KeyEvent::Up => {
                cursor = move_vertical(&text, cursor, false);
                status.clear();
                confirming_discard = false;
            }
            keyboard::KeyEvent::Down => {
                cursor = move_vertical(&text, cursor, true);
                status.clear();
                confirming_discard = false;
            }
            _ => {}
        }
        render_editor(&path, &text, cursor, &status);
    }
}

fn open_or_create_editor_file(path: &str) -> Result<Vec<u8>, FsError> {
    with_vfs(|vfs| match vfs.metadata(path) {
        Ok(metadata) if metadata.node_type == NodeType::File => read_file(vfs, path),
        Ok(_) => Err(FsError::IsDirectory),
        Err(FsError::NotFound) => {
            vfs.create_file(path)?;
            Ok(Vec::new())
        }
        Err(error) => Err(error),
    })
}

fn save_editor_file(path: &str, text: &str) -> Result<(), FsError> {
    with_vfs(|vfs| {
        if !text.is_empty() && vfs.write(path, 0, text.as_bytes())? != text.len() {
            return Err(FsError::NoSpace);
        }
        vfs.truncate(path, text.len())
    })
}

fn render_editor(path: &str, text: &str, cursor: usize, status: &str) {
    crate::terminal::terminal_clear();
    crate::println!("OCTAGRAM nano - {path}");
    crate::print!("{}", &text[..cursor]);
    crate::print!("|");
    crate::print!("{}", &text[cursor..]);
    crate::print!("\n");
    crate::println!("Ctrl+S Save   Ctrl+Q Quit   Arrow keys move");
    if !status.is_empty() {
        crate::println!("{status}");
    }
}

fn previous_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    text[..cursor]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
}

fn next_char_boundary(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .chars()
        .next()
        .map_or(cursor, |character| cursor + character.len_utf8())
}

fn move_vertical(text: &str, cursor: usize, down: bool) -> usize {
    let line_start = text[..cursor].rfind('\n').map_or(0, |index| index + 1);
    let column = text[line_start..cursor].chars().count();
    if down {
        let line_end = text[cursor..]
            .find('\n')
            .map_or(text.len(), |index| cursor + index);
        if line_end == text.len() {
            return cursor;
        }
        let next_start = line_end + 1;
        let next_end = text[next_start..]
            .find('\n')
            .map_or(text.len(), |index| next_start + index);
        position_at_column(text, next_start, next_end, column)
    } else {
        let Some(previous_end) = line_start.checked_sub(1) else {
            return cursor;
        };
        let previous_start = text[..previous_end]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        position_at_column(text, previous_start, previous_end, column)
    }
}

fn position_at_column(text: &str, start: usize, end: usize, column: usize) -> usize {
    text[start..end]
        .char_indices()
        .nth(column)
        .map_or(end, |(offset, _)| start + offset)
}

fn read_file(vfs: &mut Vfs, path: &str) -> Result<Vec<u8>, FsError> {
    let metadata = vfs.metadata(path)?;
    if metadata.node_type != NodeType::File {
        return Err(FsError::IsDirectory);
    }
    if metadata.len > MAX_FILE_OPERATION {
        return Err(FsError::NoSpace);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(metadata.len)
        .map_err(|_| FsError::NoSpace)?;
    bytes.resize(metadata.len, 0);
    let count = vfs.read(path, 0, &mut bytes)?;
    bytes.truncate(count);
    Ok(bytes)
}

fn path_argument(current_dir: &str, arguments: &str) -> Option<String> {
    let path = arguments.split_whitespace().next()?;
    resolve_path(current_dir, path)
}

fn resolve_path(current_dir: &str, path: &str) -> Option<String> {
    if path.as_bytes().contains(&0) {
        return None;
    }
    let joined = if path.starts_with('/') {
        String::from(path)
    } else {
        alloc::format!("{}/{}", current_dir.trim_end_matches('/'), path)
    };

    let mut components = Vec::new();
    for component in joined.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            _ => components.push(component),
        }
    }

    let mut normalized = String::new();
    for component in components {
        normalized.push('/');
        normalized.push_str(component);
    }
    if normalized.is_empty() {
        normalized.push('/');
    }
    Some(normalized)
}

fn print_metadata(path: &str, metadata: Metadata) {
    let kind = match metadata.node_type {
        NodeType::File => "file",
        NodeType::Directory => "directory",
        NodeType::Symlink => "symlink",
    };
    crate::println!(
        "{kind}  {:03o}  {} bytes  {path}",
        metadata.permissions,
        metadata.len
    );
}

fn with_vfs<R>(operation: impl FnOnce(&mut Vfs) -> Result<R, FsError>) -> Result<R, FsError> {
    crate::fs::with_vfs(operation).and_then(|result| result)
}

fn print_fs_error(command: &str, path: &str, error: FsError) {
    crate::println!("{command}: {path}: {error:?}");
}

#[cfg(test)]
mod tests {
    use super::{
        move_vertical, next_char_boundary, previous_char_boundary, program_candidates,
        resolve_path, split_shell_words,
    };

    #[test]
    fn resolves_relative_and_absolute_paths_from_current_directory() {
        assert_eq!(
            resolve_path("/home/user", "notes"),
            Some("/home/user/notes".into())
        );
        assert_eq!(
            resolve_path("/home/user", "/etc/config"),
            Some("/etc/config".into())
        );
        assert_eq!(resolve_path("/", "bin"), Some("/bin".into()));
    }

    #[test]
    fn normalizes_dot_segments_and_keeps_paths_inside_root() {
        assert_eq!(
            resolve_path("/home/user", ".././notes"),
            Some("/home/notes".into())
        );
        assert_eq!(resolve_path("/home", "../../../"), Some("/".into()));
    }

    #[test]
    fn searches_program_names_in_path_and_resolves_explicit_paths() {
        let paths = vec!["/bin".into(), "/mnt/bin".into()];
        assert_eq!(
            program_candidates("/home/user", "program.elf", &paths),
            Some(vec![
                "/bin/program.elf".into(),
                "/mnt/bin/program.elf".into()
            ])
        );
        assert_eq!(
            program_candidates("/home/user", "./program.elf", &paths),
            Some(vec!["/home/user/program.elf".into()])
        );
    }

    #[test]
    fn splits_quoted_program_arguments() {
        assert_eq!(
            split_shell_words(r#"one "two words" 'three' escaped\ value"#),
            Ok(vec![
                "one".into(),
                "two words".into(),
                "three".into(),
                "escaped value".into()
            ])
        );
        assert!(split_shell_words("\"unclosed").is_err());
    }

    #[test]
    fn editor_cursor_moves_between_lines_and_utf8_characters() {
        let text = "aé\nxy";
        assert_eq!(previous_char_boundary(text, 3), Some(1));
        assert_eq!(next_char_boundary(text, 1), 3);
        assert_eq!(move_vertical(text, 3, true), 6);
        assert_eq!(move_vertical(text, 5, false), 1);
    }
}
