//! Small interactive shell for the framebuffer terminal and RAM VFS.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::drivers::keyboard;
use crate::fs::{FsError, Metadata, NodeType, Vfs};
use crate::storage::{block, ext2::Ext2, fat32::Fat32};

const MAX_COMMAND_LENGTH: usize = 256;
const MAX_FILE_OPERATION: usize = 64 * 1024;
const MAX_TOOLCHAIN_FILE: usize = 4 * 1024 * 1024;
const DEFAULT_PROGRAM_PATH: &[&str] = &["/bin", "/usr/bin", "/usr/local/bin"];

pub async fn run(terminal: usize) {
    crate::terminal::set_output_terminal(terminal);
    crate::println!("OCTAGRAM-OS shell (tty{})", terminal);
    let mut current_dir = String::from("/");
    let mut program_path = DEFAULT_PROGRAM_PATH
        .iter()
        .map(|path| String::from(*path))
        .collect::<Vec<_>>();
    let mut environment = alloc::vec![String::from("PATH=/bin:/usr/bin:/usr/local/bin")];
    let mut current_user = String::from("octagram");
    print_prompt(&current_user, &current_dir);
    let mut line = String::new();

    loop {
        let event = keyboard::next_event_for(terminal).await;
        crate::terminal::set_output_terminal(terminal);
        let keyboard::KeyEvent::Character(character) = event else {
            continue;
        };
        match character {
            '\n' | '\r' => {
                crate::print!("\n");
                execute(
                    &line,
                    &mut current_dir,
                    &mut program_path,
                    &mut environment,
                    &mut current_user,
                    terminal,
                )
                .await;
                crate::terminal::set_output_terminal(terminal);
                line.clear();
                print_prompt(&current_user, &current_dir);
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

fn print_prompt(current_user: &str, current_dir: &str) {
    crate::print!("{current_user}:{current_dir} $ ");
}

async fn execute(
    line: &str,
    current_dir: &mut String,
    program_path: &mut Vec<String>,
    environment: &mut Vec<String>,
    current_user: &mut String,
    terminal: usize,
) {
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
        "export" => set_environment(current_dir, arguments, program_path, environment),
        "cd" => change_directory(current_dir, arguments),
        "echo" => echo_command(current_dir, arguments),
        "ls" => list_directory(current_dir, arguments),
        "cat" => show_file(current_dir, arguments),
        "head" => show_line_selection(current_dir, arguments, false),
        "tail" => show_line_selection(current_dir, arguments, true),
        "wc" => count_file_text(current_dir, arguments),
        "grep" => search_file_text(current_dir, arguments),
        "touch" => create_node(current_dir, arguments, false),
        "mkdir" => create_node(current_dir, arguments, true),
        "ln" => create_symlink(current_dir, arguments),
        "rm" => remove_node(current_dir, arguments),
        "chmod" => change_permissions(current_dir, arguments),
        "stat" => show_metadata(current_dir, arguments),
        "cp" => copy_file(current_dir, arguments),
        "edit" => edit_file(current_dir, arguments, terminal).await,
        "date" => show_date(),
        "shutdown" => shutdown(),
        "devices" => list_block_devices(),
        "savelog" => save_log(current_dir, arguments),
        "lsdev" => list_kernel_devices(),
        "drivers" => list_kernel_drivers(),
        "net" => run_network_command(arguments),
        "ping" => run_ping_command(arguments),
        "dns" => run_dns_command(arguments),
        "http" | "curl" => run_http_command(arguments),
        "mount" => mount_device(current_dir, arguments),
        "umount" => unmount_device(current_dir, arguments),
        "df" => list_mounts(),
        "persist" => configure_persistent_home(current_dir, arguments),
        "apps" => list_apps(),
        "pkg" => manage_package(current_dir, arguments),
        "login" => login_user(arguments, current_user),
        "whoami" => crate::println!("{current_user}"),
        "session" => crate::println!("session user={current_user} tty=tty{terminal}"),
        "sched" => show_scheduler_status(),
        "passwd" => crate::println!("passwd: password authentication is not implemented"),
        "mkfs.fat" => format_fat32_device(arguments),
        "as" => run_toolchain_command("as", current_dir, arguments),
        "ld" => run_toolchain_command("ld", current_dir, arguments),
        "cc" => run_toolchain_command("cc", current_dir, arguments),
        "run" => run_program(current_dir, arguments, environment, terminal),
        "runp" => run_parallel_programs(current_dir, arguments, environment, terminal),
        _ => run_external_program(
            current_dir,
            command,
            arguments,
            program_path,
            environment,
            terminal,
        ),
    }
}

/// Tampilkan status semua task terjadwal; baris `WATCHDOG` berarti umur tunggu
/// proses runnable melewati ambang anti-starvasi.
fn show_scheduler_status() {
    let entries = crate::elf::scheduler_status();
    if entries.is_empty() {
        crate::println!("sched: no user processes");
        return;
    }
    crate::println!("  PID  TID   NICE POLICY   AGE  STATE");
    for entry in entries {
        let state = if entry.exited {
            "exited"
        } else if entry.waiting {
            "waiting"
        } else {
            "runnable"
        };
        let watchdog = if crate::elf::scheduler_watchdog_tripped(entry.age) {
            " WATCHDOG"
        } else {
            ""
        };
        let kind = if entry.is_thread { " t" } else { "" };
        crate::println!(
            "{:>5} {:>4} {:>6} {:>6} {:>5}  {}{}{}",
            entry.pid,
            entry.tid,
            entry.priority,
            entry.policy,
            entry.age,
            state,
            kind,
            watchdog
        );
    }
}

fn print_help() {
    crate::println!("Commands:");
    crate::println!("  help                   show this help");
    crate::println!("  clear                  clear the screen");
    crate::println!("  savelog <path>         save the kernel log to a file (e.g. /mnt/LOG.TXT)");
    crate::println!("  pwd                    show the current directory");
    crate::println!("  path                   show executable search directories");
    crate::println!("  export NAME=value     set an environment variable");
    crate::println!("  cd [path]              change directory (default: /)");
    crate::println!("  ls [path]              list a directory");
    crate::println!("  cat <path>             print a UTF-8 file");
    crate::println!("  head [-n count] <file> print the first lines of a text file");
    crate::println!("  tail [-n count] <file> print the last lines of a text file");
    crate::println!("  wc <file>              count lines, words, and bytes");
    crate::println!("  grep [-n] <text> <file> search literal text in a file");
    crate::println!("  touch <path>           create an empty file");
    crate::println!("  mkdir <path>           create a directory");
    crate::println!("  ln -s <target> <link>  create a symbolic link");
    crate::println!("  rm <path>              remove an empty file or directory");
    crate::println!("  chmod <mode> <path>    set RAMFS permissions (e.g. +x, 755)");
    crate::println!("  stat <path>            show file metadata");
    crate::println!("  cp <source> <target>   copy a file");
    crate::println!("  edit <path>            edit a UTF-8 file");
    crate::println!("  date                   show date and time from the RTC");
    crate::println!("  shutdown               power off using ACPI");
    crate::println!("  sched                  show scheduler priorities, aging, and watchdog flags");
    crate::println!("  devices                list detected storage devices");
    crate::println!("  lsdev                  list bus devices and bound drivers");
    crate::println!("  drivers                list registered kernel drivers");
    crate::println!("  net status|dhcp        inspect interface or request an IPv4 lease");
    crate::println!("  net tcp <host> <port> <text> send TCP payload and print response");
    crate::println!("  net udp <IP> <port> <text> send one UDP datagram");
    crate::println!("  dns <hostname>         resolve domain name to IPv4");
    crate::println!("  http <url>             make HTTP GET request (curl also works)");
    crate::println!("  ping <IPv4 | host>     send an ICMP echo request");
    crate::println!("  mount <device> <path> [fat32|ext2]  mount a filesystem read-write");
    crate::println!("  umount <path>          unmount a filesystem");
    crate::println!("  df                     list mounted filesystem roots");
    crate::println!("  persist <mount>        prepare /home/octagram on a mounted filesystem");
    crate::println!("  apps                   list native apps under /apps");
    crate::println!("  pkg list|info|add|update|remove|verify  manage native packages");
    crate::println!("  login [octagram|root]  start a local shell session");
    crate::println!("  whoami                 show current session user");
    crate::println!("  session                show current session");
    crate::println!("  mkfs.fat <device> --yes  format a FAT32 superfloppy (erases data)");
    crate::println!("  as <input.s> -o <out.o> assemble supported x86_64 source");
    crate::println!("  ld <input.o> -o <out.elf> link a native toolchain object");
    crate::println!("  cc <input.c> -o <out.elf> compile the kernel C subset");
    crate::println!("  tcc <input.c> [-o <out.elf>] compile C with TinyCC");
    crate::println!("  echo <text>            print text");
    crate::println!("  run <path>             run a static x86_64 ELF");
    crate::println!("  runp <elf> <elf>...    run multiple ELFs with timer preemption");
    crate::println!("  <program>              run a program found in PATH");
}

fn show_date() {
    match crate::drivers::rtc::read() {
        Ok(time) => crate::println!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            time.year,
            time.month,
            time.day,
            time.hour,
            time.minute,
            time.second
        ),
        Err(error) => crate::println!("date: RTC read failed: {error:?}"),
    }
}

fn shutdown() {
    crate::println!("shutdown: requesting ACPI power-off");
    if let Err(error) = crate::drivers::acpi::power_off() {
        crate::println!("shutdown: power-off failed: {error:?}");
    }
}

fn run_network_command(arguments: &str) {
    match arguments.split_whitespace().next() {
        None | Some("status") => crate::network::print_status(),
        Some("dhcp") => {
            if let Err(error) = crate::network::start_dhcp() {
                crate::println!("net dhcp: {error}");
            }
        }
        Some("dns") => {
            let mut parts = arguments.split_whitespace();
            let _ = parts.next();
            run_dns_command(parts.next().unwrap_or(""));
        }
        Some("tcp") => run_network_tcp_command(arguments),
        Some("udp") => run_network_udp_command(arguments),
        Some("wifi") => run_wifi_command(arguments),
        Some(_) => crate::println!("usage: net [status|dhcp|dns <host>|tcp <IPv4|host> <port> <text>|udp <IPv4> <port> <text>|wifi <scan|connect <ssid>>]"),
    }
}

fn run_wifi_command(arguments: &str) {
    if !crate::drivers::qca9377::QCA9377_ENABLED {
        crate::println!(
            "net wifi: the QCA9377 driver is disabled in this build; use USB tethering (RNDIS/CDC-NCM) instead"
        );
        return;
    }
    let mut parts = arguments.split_whitespace();
    let _ = parts.next(); // "wifi"
    match parts.next() {
        None | Some("help") => {
            crate::println!("wifi subcommands:");
            crate::println!("  net wifi scan              - scan all 2.4 GHz channels");
            crate::println!("  net wifi connect <ssid>    - connect to AP by SSID");
            crate::println!("  net wifi status            - show current link state");
            crate::println!("  net wifi bmi               - re-run BMI and dump host-interest words");
            crate::println!("  net wifi retry             - restart the driver after a failed bring-up");
        }
        Some("retry") => {
            if crate::drivers::qca9377::is_initialized() {
                crate::println!("net wifi retry: the QCA9377 link is already up");
            } else if crate::drivers::qca9377::request_restart() {
                crate::println!(
                    "net wifi retry: the driver will restart the WLAN device from the idle loop"
                );
            } else {
                crate::println!("net wifi retry: no Qualcomm Atheros WLAN function found");
            }
        }
        Some("bmi") => {
            if let Err(error) = unsafe { crate::drivers::qca9377::run_bmi_diagnostics() } {
                crate::println!("net wifi bmi: {error}");
            }
        }
        Some("status") => {
            crate::network::print_status();
        }
        Some("scan") => {
            crate::println!("Starting WiFi scan on 2.4 GHz channels...");
            crate::network::wifi_scan();
        }
        Some("connect") => {
            let ssid = parts.next().unwrap_or("").trim();
            if ssid.is_empty() {
                crate::println!("usage: net wifi connect <ssid>");
                return;
            }
            crate::println!("Connecting to SSID: {}", ssid);
            crate::network::wifi_connect(ssid);
        }
        Some(sub) => {
            crate::println!(
                "net wifi: unknown subcommand '{sub}'. Try: scan, connect, status, bmi, retry"
            );
        }
    }
}

fn run_network_udp_command(arguments: &str) {
    let mut fields = arguments.split_whitespace();
    let _ = fields.next();
    let (Some(address), Some(port)) = (fields.next(), fields.next()) else {
        crate::println!("usage: net udp <IPv4> <port> <text>");
        return;
    };
    let payload = fields.collect::<Vec<_>>().join(" ");
    if payload.is_empty() {
        crate::println!("usage: net udp <IPv4> <port> <text>");
        return;
    }
    let Some(address) = parse_ipv4_address(address) else {
        crate::println!("net udp: invalid IPv4 address");
        return;
    };
    let Ok(port) = port.parse::<u16>() else {
        crate::println!("net udp: invalid port");
        return;
    };
    match crate::network::send_udp_ipv4(address, 49152, port, payload.as_bytes()) {
        Ok(()) => crate::println!("UDP datagram sent ({} bytes)", payload.len()),
        Err(error) => crate::println!("net udp: {error}"),
    }
}

fn run_network_tcp_command(arguments: &str) {
    let mut fields = arguments.split_whitespace();
    let _ = fields.next(); // "tcp"
    let (Some(target), Some(port)) = (fields.next(), fields.next()) else {
        crate::println!("usage: net tcp <IPv4|hostname> <port> <text>");
        return;
    };
    let payload = fields.collect::<Vec<_>>().join(" ");
    if payload.is_empty() {
        crate::println!("usage: net tcp <IPv4|hostname> <port> <text>");
        return;
    }
    let Ok(port) = port.parse::<u16>() else {
        crate::println!("net tcp: invalid port");
        return;
    };

    let ip = match parse_ipv4_address(target) {
        Some(ip) => ip,
        None => match crate::network::dns_resolve(target) {
            Ok(ip) => ip,
            Err(e) => {
                crate::println!("net tcp: cannot resolve '{target}': {e}");
                return;
            }
        },
    };

    crate::println!("Connecting to {}.{}.{}.{}:{}...", ip[0], ip[1], ip[2], ip[3], port);
    match crate::network::tcp_exchange(ip, port, payload.as_bytes(), 5000) {
        Ok(resp) => {
            if resp.is_empty() {
                crate::println!("TCP: sent {} bytes, connection closed without response", payload.len());
            } else {
                crate::println!("TCP response ({} bytes):", resp.len());
                if let Ok(text) = core::str::from_utf8(&resp) {
                    crate::println!("{text}");
                } else {
                    crate::println!("<binary response: {} bytes>", resp.len());
                }
            }
        }
        Err(err) => crate::println!("net tcp: {err}"),
    }
}

fn run_dns_command(arguments: &str) {
    let hostname = arguments.trim();
    if hostname.is_empty() {
        crate::println!("usage: dns <hostname>");
        return;
    }
    match crate::network::dns_resolve(hostname) {
        Ok(ip) => {
            crate::println!("{hostname} has address {}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);
        }
        Err(err) => {
            crate::println!("dns: cannot resolve '{hostname}': {err}");
        }
    }
}

fn run_http_command(arguments: &str) {
    let url = arguments.trim();
    if url.is_empty() {
        crate::println!("usage: http <URL> (or curl <URL>)");
        return;
    }
    crate::println!("Fetching {url}...");
    match crate::network::http_get(url) {
        Ok(resp) => {
            crate::println!("HTTP Status: {}", resp.status_code);
            for (k, v) in &resp.headers {
                crate::println!("{k}: {v}");
            }
            crate::println!("");
            crate::println!("{}", resp.body_str());
        }
        Err(err) => crate::println!("http error: {err}"),
    }
}

fn run_ping_command(arguments: &str) {
    let target = arguments.trim();
    if target.is_empty() {
        crate::println!("usage: ping <IPv4 address | hostname>");
        return;
    }
    let ip = match parse_ipv4_address(target) {
        Some(ip) => ip,
        None => match crate::network::dns_resolve(target) {
            Ok(ip) => ip,
            Err(e) => {
                crate::println!("ping: cannot resolve '{target}': {e}");
                return;
            }
        },
    };
    if let Err(error) = crate::network::ping(ip) {
        crate::println!("ping: {error}");
    }
}

fn parse_ipv4_address(value: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut parts = value.split('.');
    for octet in &mut octets {
        *octet = parts.next()?.parse().ok()?;
    }
    parts.next().is_none().then_some(octets)
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

/// Save everything printed since boot to a file, for example on a USB stick:
/// `mount usb0 /mnt`, `savelog /mnt/LOG.TXT`, `umount /mnt`.
fn save_log(current_dir: &str, arguments: &str) {
    let Some(path) = path_argument(current_dir, arguments) else {
        crate::println!("usage: savelog <path>   (example: savelog /mnt/LOG.TXT)");
        return;
    };
    // Snapshot first: printing or writing while the log lock is held would
    // deadlock, and the file write must not be part of what is being saved.
    let data = crate::klog::snapshot();
    let result = with_vfs(|vfs| -> Result<usize, FsError> {
        match vfs.metadata(&path) {
            Ok(_) => vfs.truncate(&path, 0)?,
            Err(FsError::NotFound) => vfs.create_file(&path)?,
            Err(error) => return Err(error),
        }
        let mut offset = 0;
        while offset < data.len() {
            let end = (offset + 4096).min(data.len());
            let written = vfs.write(&path, offset, &data[offset..end])?;
            if written == 0 {
                return Err(FsError::IoError);
            }
            offset += written;
        }
        Ok(offset)
    });
    match result {
        Ok(bytes) => crate::println!("savelog: wrote {bytes} bytes to {path} (run umount before removing the USB stick)"),
        Err(error) => print_fs_error("savelog", &path, error),
    }
}

/// `echo text`, `echo text > file` (truncate) and `echo text >> file` (append).
fn echo_command(current_dir: &str, arguments: &str) {
    let Some(position) = arguments.find('>') else {
        crate::println!("{arguments}");
        return;
    };
    let append = arguments[position + 1..].starts_with('>');
    let redirect_len = if append { 2 } else { 1 };
    let target = arguments[position + redirect_len..].trim();
    let mut text = arguments[..position].trim();
    for quote in ['"', '\''] {
        if text.len() >= 2 && text.starts_with(quote) && text.ends_with(quote) {
            text = &text[1..text.len() - 1];
            break;
        }
    }
    let Some(path) = path_argument(current_dir, target) else {
        crate::println!("usage: echo <text> [> | >>] <path>");
        return;
    };
    let mut line = String::new();
    if line.try_reserve(text.len() + 1).is_err() {
        print_fs_error("echo", &path, FsError::NoSpace);
        return;
    }
    line.push_str(text);
    line.push('\n');
    let result = with_vfs(|vfs| -> Result<(), FsError> {
        let offset = match vfs.metadata(&path) {
            Ok(metadata) if metadata.node_type == NodeType::File => {
                if append {
                    metadata.len
                } else {
                    vfs.truncate(&path, 0)?;
                    0
                }
            }
            Ok(_) => return Err(FsError::IsDirectory),
            Err(FsError::NotFound) => {
                vfs.create_file(&path)?;
                0
            }
            Err(error) => return Err(error),
        };
        if vfs.write(&path, offset, line.as_bytes())? != line.len() {
            return Err(FsError::NoSpace);
        }
        Ok(())
    });
    if let Err(error) = result {
        print_fs_error("echo", &path, error);
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

fn show_line_selection(current_dir: &str, arguments: &str, from_end: bool) {
    let command = if from_end { "tail" } else { "head" };
    let words = match split_shell_words(arguments) {
        Ok(words) => words,
        Err(()) => {
            crate::println!("{command}: unmatched quote or escape");
            return;
        }
    };
    let (count, path_word) = match parse_line_selection_arguments(&words) {
        Ok(arguments) => arguments,
        Err(()) => {
            crate::println!("usage: {command} [-n count] <file>");
            return;
        }
    };
    let Some(path) = resolve_path(current_dir, path_word) else {
        crate::println!("{command}: invalid path");
        return;
    };
    let Some(text) = read_text_file(command, &path) else {
        return;
    };
    let line_count = text.lines().count();
    let range = selected_line_range(line_count, count, from_end);
    for line in text.lines().skip(range.start).take(range.len()) {
        crate::println!("{line}");
    }
}

fn count_file_text(current_dir: &str, arguments: &str) {
    let words = match split_shell_words(arguments) {
        Ok(words) if words.len() == 1 => words,
        Ok(_) => {
            crate::println!("usage: wc <file>");
            return;
        }
        Err(()) => {
            crate::println!("wc: unmatched quote or escape");
            return;
        }
    };
    let Some(path) = resolve_path(current_dir, &words[0]) else {
        crate::println!("wc: invalid path");
        return;
    };
    let Some(text) = read_text_file("wc", &path) else {
        return;
    };
    let (lines, words, bytes) = text_counts(&text);
    crate::println!("{lines} {words} {bytes} {path}");
}

fn search_file_text(current_dir: &str, arguments: &str) {
    let words = match split_shell_words(arguments) {
        Ok(words) => words,
        Err(()) => {
            crate::println!("grep: unmatched quote or escape");
            return;
        }
    };
    let (show_line_numbers, pattern, file) = match parse_grep_arguments(&words) {
        Ok(arguments) => arguments,
        Err(()) => {
            crate::println!("usage: grep [-n] <text> <file>");
            return;
        }
    };
    let Some(path) = resolve_path(current_dir, file) else {
        crate::println!("grep: invalid path");
        return;
    };
    let Some(text) = read_text_file("grep", &path) else {
        return;
    };
    for (index, line) in matching_lines(&text, pattern) {
        if show_line_numbers {
            crate::println!("{}:{line}", index + 1);
        } else {
            crate::println!("{line}");
        }
    }
}

fn parse_line_selection_arguments(words: &[String]) -> Result<(usize, &str), ()> {
    match words {
        [path] => Ok((10, path)),
        [option, count, path] if option == "-n" => {
            Ok((count.parse::<usize>().map_err(|_| ())?, path))
        }
        _ => Err(()),
    }
}

fn parse_grep_arguments(words: &[String]) -> Result<(bool, &str, &str), ()> {
    let (show_line_numbers, pattern_index) = if words.first().is_some_and(|word| word == "-n") {
        (true, 1)
    } else {
        (false, 0)
    };
    if words.len() != pattern_index + 2 {
        return Err(());
    }
    Ok((
        show_line_numbers,
        &words[pattern_index],
        &words[pattern_index + 1],
    ))
}

fn matching_lines<'a>(text: &'a str, pattern: &'a str) -> impl Iterator<Item = (usize, &'a str)> {
    text.lines()
        .enumerate()
        .filter(move |(_, line)| line.contains(pattern))
}

fn read_text_file(command: &str, path: &str) -> Option<String> {
    match with_vfs(|vfs| read_file(vfs, path)) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Some(text),
            Err(_) => {
                crate::println!("{command}: {path}: file is not valid UTF-8");
                None
            }
        },
        Err(error) => {
            print_fs_error(command, path, error);
            None
        }
    }
}

fn selected_line_range(line_count: usize, count: usize, from_end: bool) -> core::ops::Range<usize> {
    if from_end {
        line_count.saturating_sub(count)..line_count
    } else {
        0..line_count.min(count)
    }
}

fn text_counts(text: &str) -> (usize, usize, usize) {
    let lines = text.bytes().filter(|byte| *byte == b'\n').count();
    let words = text.split_whitespace().count();
    (lines, words, text.len())
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
        let data = read_file_limited(vfs, &source, MAX_TOOLCHAIN_FILE)?;
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

fn run_program(current_dir: &str, arguments: &str, environment: &[String], terminal: usize) {
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
    let result = crate::elf::run_file_from_with_args_and_environment_on_terminal(
        &path,
        current_dir,
        &words,
        environment,
        terminal,
    );
    crate::terminal::set_output_terminal(terminal);
    match result {
        Ok(status) => crate::println!("program exited with status {status}"),
        Err(error) => crate::println!("run: {path}: {error:?}"),
    }
}

fn run_parallel_programs(
    current_dir: &str,
    arguments: &str,
    environment: &[String],
    terminal: usize,
) {
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
    let result = crate::elf::run_files_parallel_from_with_environment_on_terminal(
        &paths,
        current_dir,
        environment,
        terminal,
    );
    crate::terminal::set_output_terminal(terminal);
    match result {
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

fn set_environment(
    current_dir: &str,
    arguments: &str,
    program_path: &mut Vec<String>,
    environment: &mut Vec<String>,
) {
    let words = match split_shell_words(arguments) {
        Ok(words) if words.len() == 1 => words,
        _ => {
            crate::println!("usage: export NAME=value");
            return;
        }
    };
    let Some((name, value)) = words[0].split_once('=') else {
        crate::println!("usage: export NAME=value");
        return;
    };
    if !crate::elf::valid_environment_key(name) {
        crate::println!("export: invalid variable name");
        return;
    }

    let mut normalized_paths = None;
    let mut normalized_value = String::new();
    if normalized_value.try_reserve(value.len()).is_err() {
        crate::println!("export: out of memory");
        return;
    }
    normalized_value.push_str(value);
    if name == "PATH" {
        let mut paths = Vec::new();
        for path in value.split(':').filter(|path| !path.is_empty()) {
            let Some(path) = resolve_path(current_dir, path) else {
                crate::println!("export: invalid PATH entry");
                return;
            };
            if !paths.contains(&path) {
                if paths.try_reserve(1).is_err() {
                    crate::println!("export: out of memory");
                    return;
                }
                paths.push(path);
            }
        }
        let value_length = paths
            .iter()
            .enumerate()
            .try_fold(0usize, |length, (index, path)| {
                length
                    .checked_add(path.len())
                    .and_then(|length| length.checked_add(usize::from(index != 0)))
            });
        let Some(value_length) = value_length else {
            crate::println!("export: value too long");
            return;
        };
        normalized_value.clear();
        if normalized_value.try_reserve(value_length).is_err() {
            crate::println!("export: out of memory");
            return;
        }
        for (index, path) in paths.iter().enumerate() {
            if index != 0 {
                normalized_value.push(':');
            }
            normalized_value.push_str(path);
        }
        normalized_paths = Some(paths);
    }

    let mut assignment = String::new();
    if assignment
        .try_reserve(name.len() + 1 + normalized_value.len())
        .is_err()
    {
        crate::println!("export: out of memory");
        return;
    }
    assignment.push_str(name);
    assignment.push('=');
    assignment.push_str(&normalized_value);
    let existing = environment.iter().position(|entry| {
        entry.starts_with(name) && entry.as_bytes().get(name.len()) == Some(&b'=')
    });
    if existing.is_none() && environment.try_reserve(1).is_err() {
        crate::println!("export: out of memory");
        return;
    }
    if let Some(existing) = existing {
        environment[existing] = assignment;
    } else {
        environment.push(assignment);
    }
    if let Some(paths) = normalized_paths {
        *program_path = paths;
    }
}

fn run_external_program(
    current_dir: &str,
    command: &str,
    arguments: &str,
    program_path: &[String],
    environment: &[String],
    terminal: usize,
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
                let result = crate::elf::run_file_from_with_args_and_environment_on_terminal(
                    &path,
                    current_dir,
                    &argv,
                    environment,
                    terminal,
                );
                crate::terminal::set_output_terminal(terminal);
                match result {
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

fn list_kernel_devices() {
    let devices = crate::drivers::device_model::devices();
    if devices.is_empty() {
        crate::println!("no bus devices registered");
        return;
    }

    for device in devices {
        let driver = device.driver_name.unwrap_or("unbound");
        match device.key {
            crate::drivers::device_model::DeviceKey::Pci {
                bus,
                device,
                function,
            } => crate::println!(
                "pci {:02x}:{:02x}.{}  driver={}",
                bus,
                device,
                function,
                driver
            ),
            crate::drivers::device_model::DeviceKey::UsbInterface {
                controller,
                slot_id,
                interface_number,
            } => crate::println!(
                "usb {:?}#{} slot={} interface={}  driver={}",
                controller.kind,
                controller.instance,
                slot_id,
                interface_number,
                driver
            ),
        }
    }
}

fn format_fat32_device(arguments: &str) {
    let mut parts = arguments.split_whitespace();
    let (Some(device_name), Some(confirmation), None) = (parts.next(), parts.next(), parts.next())
    else {
        crate::println!("usage: mkfs.fat <device> --yes");
        return;
    };
    if confirmation != "--yes" {
        crate::println!("mkfs.fat: formatting erases existing filesystem data");
        crate::println!("rerun with `mkfs.fat {device_name} --yes` to confirm");
        return;
    }
    let Some(device) = block::get(device_name) else {
        crate::println!("mkfs.fat: storage device `{device_name}` not found (try `devices`)");
        return;
    };

    crate::println!("Formatting {device_name} as FAT32 superfloppy...");
    match Fat32::format(device.as_ref()) {
        Ok(()) => crate::println!("Formatted {device_name} as FAT32"),
        Err(error) => crate::println!("mkfs.fat: {device_name}: {error:?}"),
    }
}

fn list_kernel_drivers() {
    let drivers = crate::drivers::device_model::drivers();
    if drivers.is_empty() {
        crate::println!("no kernel drivers registered");
        return;
    }
    for driver in drivers {
        crate::println!("{driver}");
    }
}

fn mount_device(current_dir: &str, arguments: &str) {
    let mut parts = arguments.split_whitespace();
    let (Some(device_name), Some(mountpoint)) = (parts.next(), parts.next()) else {
        crate::println!("usage: mount <device> <path> [fat32|ext2]");
        return;
    };
    let filesystem_type = parts.next().unwrap_or("fat32");
    if parts.next().is_some() || !matches!(filesystem_type, "fat32" | "ext2") {
        crate::println!("usage: mount <device> <path> [fat32|ext2]");
        return;
    }
    let Some(mountpoint) = resolve_path(current_dir, mountpoint) else {
        crate::println!("mount: invalid mount path");
        return;
    };
    let Some(device) = block::get(device_name) else {
        crate::println!("mount: storage device `{device_name}` not found (try `devices`)");
        return;
    };
    if filesystem_type == "ext2" {
        match Ext2::mount(device) {
            Ok(filesystem) => match with_vfs(|vfs| vfs.mount(&mountpoint, filesystem)) {
                Ok(()) => {
                    crate::println!("mounted {device_name} at {mountpoint} (ext2, read-write)")
                }
                Err(error) => crate::println!("mount: {mountpoint}: {error:?}"),
            },
            Err(error) => crate::println!("mount: {device_name}: {error:?}"),
        }
    } else {
        match Fat32::mount(device) {
            Ok(filesystem) => match with_vfs(|vfs| vfs.mount(&mountpoint, filesystem)) {
                Ok(()) => {
                    crate::println!("mounted {device_name} at {mountpoint} (FAT32, read-write)")
                }
                Err(error) => crate::println!("mount: {mountpoint}: {error:?}"),
            },
            Err(error) => crate::println!("mount: {device_name}: {error:?}"),
        }
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

fn list_mounts() {
    match with_vfs(|vfs| Ok(vfs.mounts().to_vec())) {
        Ok(mounts) => {
            if mounts.is_empty() {
                crate::println!("no mounted filesystems");
                return;
            }
            crate::println!("Mount point       Filesystem");
            for (mountpoint, filesystem) in mounts {
                crate::println!("{:<18} {}", mountpoint, filesystem);
            }
        }
        Err(error) => crate::println!("df: {error:?}"),
    }
}

fn valid_package_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn package_registry_has(registry: &str, name: &str) -> bool {
    registry.lines().any(|line| {
        line.split('|')
            .next()
            .is_some_and(|registered| registered == name)
    })
}

fn package_remove_registry(registry: &str, name: &str) -> String {
    let mut output = String::new();
    for line in registry.lines() {
        if line.is_empty() {
            continue;
        }
        if line
            .split('|')
            .next()
            .is_some_and(|registered| registered == name)
        {
            continue;
        }
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(line);
    }
    if !output.is_empty() {
        output.push('\n');
    }
    output
}

fn manage_package(current_dir: &str, arguments: &str) {
    let mut parts = arguments.split_whitespace();
    let Some(operation) = parts.next() else {
        crate::println!("usage: pkg list|info|add|update|remove|verify");
        return;
    };
    match operation {
        "list" => {
            match with_vfs(|vfs| {
                let data = read_file_limited(
                    vfs,
                    "/var/lib/octagram/packages/registry.txt",
                    MAX_FILE_OPERATION,
                )?;
                Ok(data)
            }) {
                Ok(bytes) => {
                    let text = core::str::from_utf8(&bytes)
                        .unwrap_or("# invalid package registry");
                    if text.is_empty() {
                        crate::println!("no packages installed");
                    } else {
                        for line in text.lines() {
                            let mut fields = line.split('|');
                            if let (Some(name), Some(version), Some(path), Some(size)) =
                                (fields.next(), fields.next(), fields.next(), fields.next())
                            {
                                crate::println!("{name} {version} {path} size={size}");
                            }
                        }
                    }
                }
                Err(error) => crate::println!("pkg: {error:?}"),
            }
        }
        "info" => {
            let Some(name) = parts.next() else {
                crate::println!("usage: pkg info <name>");
                return;
            };
            if !valid_package_name(name) {
                crate::println!("pkg: invalid package name");
                return;
            }
            match with_vfs(|vfs| {
                read_file_limited(
                    vfs,
                    &format!("/var/lib/octagram/packages/{name}.pkg"),
                    MAX_FILE_OPERATION,
                )
            }) {
                Ok(data) => match core::str::from_utf8(&data) {
                    Ok(text) => crate::print!("{text}"),
                    Err(_) => crate::println!("pkg: invalid metadata"),
                },
                Err(error) => crate::println!("pkg info: {name}: {error:?}"),
            }
        }
        "add" | "update" => {
            let (Some(name), Some(source), version) =
                (parts.next(), parts.next(), parts.next())
            else {
                crate::println!("usage: pkg {operation} <name> <source> [version]");
                return;
            };
            if parts.next().is_some() {
                crate::println!("usage: pkg {operation} <name> <source> [version]");
                return;
            }
            if !valid_package_name(name) {
                crate::println!("pkg: invalid package name");
                return;
            }
            let Some(source) = resolve_path(current_dir, source) else {
                crate::println!("pkg: invalid source path");
                return;
            };
            let version = version.unwrap_or("0");
            let app_path = format!("/apps/{name}");
            let metadata_path = format!("/var/lib/octagram/packages/{name}.pkg");
            match with_vfs(|vfs| {
                let data = read_file_limited(vfs, &source, MAX_TOOLCHAIN_FILE)?;
                if operation == "update" {
                    if vfs.metadata(&app_path).is_ok() {
                        vfs.remove(&app_path)?;
                    }
                    if vfs.metadata(&metadata_path).is_ok() {
                        vfs.remove(&metadata_path)?;
                    }
                } else if vfs.metadata(&app_path).is_ok() {
                    return Err(FsError::AlreadyExists);
                }
                vfs.create_file(&app_path)?;
                let written = vfs.write(&app_path, 0, &data)?;
                if written != data.len() {
                    vfs.remove(&app_path)?;
                    return Err(FsError::NoSpace);
                }
                vfs.set_permissions(&app_path, 0o755)?;
                vfs.create_file(&metadata_path)?;
                let metadata = format!(
                    "name={name}\nversion={version}\npath={app_path}\nsize={}\n",
                    data.len()
                );
                vfs.write(&metadata_path, 0, metadata.as_bytes())?;
                let registry_bytes = read_file_limited(
                    vfs,
                    "/var/lib/octagram/packages/registry.txt",
                    MAX_FILE_OPERATION,
                )
                .unwrap_or_default();
                let registry = core::str::from_utf8(&registry_bytes).unwrap_or("");
                let registry = package_remove_registry(registry, name);
                let entry = format!("{name}|{version}|{app_path}|{}\n", data.len());
                let mut updated = String::from(registry);
                updated.push_str(&entry);
                match vfs.metadata("/var/lib/octagram/packages/registry.txt") {
                    Ok(_) => vfs.remove("/var/lib/octagram/packages/registry.txt")?,
                    Err(FsError::NotFound) => {}
                    Err(error) => return Err(error),
                }
                vfs.create_file("/var/lib/octagram/packages/registry.txt")?;
                vfs.write(
                    "/var/lib/octagram/packages/registry.txt",
                    0,
                    updated.as_bytes(),
                )?;
                Ok(data.len())
            }) {
                Ok(size) => crate::println!("pkg {operation}: {name} {version} size={size}"),
                Err(error) => crate::println!("pkg {operation}: {name}: {error:?}"),
            }
        }
        "remove" => {
            let Some(name) = parts.next() else {
                crate::println!("usage: pkg remove <name>");
                return;
            };
            if !valid_package_name(name) {
                crate::println!("pkg: invalid package name");
                return;
            }
            match with_vfs(|vfs| {
                let app_path = format!("/apps/{name}");
                let metadata_path = format!("/var/lib/octagram/packages/{name}.pkg");
                if vfs.metadata(&app_path).is_ok() {
                    vfs.remove(&app_path)?;
                }
                if vfs.metadata(&metadata_path).is_ok() {
                    vfs.remove(&metadata_path)?;
                }
                let registry_bytes = read_file_limited(
                    vfs,
                    "/var/lib/octagram/packages/registry.txt",
                    MAX_FILE_OPERATION,
                )?;
                let registry = core::str::from_utf8(&registry_bytes).unwrap_or("");
                let updated = package_remove_registry(registry, name);
                vfs.remove("/var/lib/octagram/packages/registry.txt")?;
                vfs.create_file("/var/lib/octagram/packages/registry.txt")?;
                vfs.write(
                    "/var/lib/octagram/packages/registry.txt",
                    0,
                    updated.as_bytes(),
                )?;
                Ok(())
            }) {
                Ok(()) => crate::println!("pkg remove: {name}"),
                Err(error) => crate::println!("pkg remove: {name}: {error:?}"),
            }
        }
        "verify" => {
            let Some(name) = parts.next() else {
                crate::println!("usage: pkg verify <name>");
                return;
            };
            if !valid_package_name(name) {
                crate::println!("pkg: invalid package name");
                return;
            }
            match with_vfs(|vfs| {
                let app_path = format!("/apps/{name}");
                let metadata_path = format!("/var/lib/octagram/packages/{name}.pkg");
                let app = read_file_limited(vfs, &app_path, MAX_TOOLCHAIN_FILE)?;
                let metadata = read_file_limited(vfs, &metadata_path, MAX_FILE_OPERATION)?;
                let metadata = core::str::from_utf8(&metadata)
                    .map_err(|_| FsError::InvalidData)?;
                let declared_size = metadata.lines()
                    .find_map(|line| line.strip_prefix("size="))
                    .and_then(|value| value.parse::<usize>().ok())
                    .ok_or(FsError::InvalidData)?;
                if declared_size != app.len() {
                    return Err(FsError::InvalidData);
                }
                let registry = read_file_limited(
                    vfs,
                    "/var/lib/octagram/packages/registry.txt",
                    MAX_FILE_OPERATION,
                )?;
                let registry = core::str::from_utf8(&registry)
                    .map_err(|_| FsError::InvalidData)?;
                if !package_registry_has(registry, name) {
                    return Err(FsError::NotFound);
                }
                Ok(app.len())
            }) {
                Ok(size) => crate::println!("pkg verify: {name} OK size={size}"),
                Err(error) => crate::println!("pkg verify: {name}: {error:?}"),
            }
        }
        _ => crate::println!("usage: pkg list|info|add|update|remove|verify"),
    }
}

fn login_user(arguments: &str, current_user: &mut String) {
    let mut parts = arguments.split_whitespace();
    let name = parts.next().unwrap_or("octagram");
    if parts.next().is_some() || (name != "octagram" && name != "root") {
        crate::println!("login: unknown or unsupported user");
        return;
    }
    current_user.clear();
    current_user.push_str(name);
    crate::println!("login: session started for {name}");
}

fn list_apps() {
    match with_vfs(|vfs| vfs.read_dir("/apps")) {
        Ok(entries) => {
            if entries.is_empty() {
                crate::println!("no apps installed");
                return;
            }
            for entry in entries {
                crate::println!("/apps/{}", entry.name);
            }
        }
        Err(error) => crate::println!("apps: {error:?}"),
    }
}

fn configure_persistent_home(current_dir: &str, arguments: &str) {
    let mut parts = arguments.split_whitespace();
    let (Some(mount), None) = (parts.next(), parts.next()) else {
        crate::println!("usage: persist <mount>");
        return;
    };
    let Some(mount) = resolve_path(current_dir, mount) else {
        crate::println!("persist: invalid mount path");
        return;
    };
    if mount == "/" {
        crate::println!("persist: refuse RAM root; mount a storage filesystem first");
        return;
    }
    let persistent_home = format!("{mount}/octagram");
    match with_vfs(|vfs| {
        if vfs.metadata(&mount).is_err() {
            return Err(FsError::NotMounted);
        }
        if vfs.metadata(&persistent_home).is_err() {
            vfs.create_dir(&persistent_home)?;
        }
        match vfs.symlink_metadata("/home/octagram") {
            Ok(metadata) if metadata.node_type == NodeType::Symlink => {}
            Ok(_) => {
                vfs.remove("/home/octagram")?;
            }
            Err(FsError::NotFound) => {}
            Err(error) => return Err(error),
        }
        vfs.symlink(&persistent_home, "/home/octagram")?;
        Ok(())
    }) {
        Ok(()) => crate::println!("persistent home configured at {persistent_home}"),
        Err(error) => crate::println!("persist: {persistent_home}: {error:?}"),
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

async fn edit_file(current_dir: &str, arguments: &str, terminal: usize) {
    crate::terminal::set_output_terminal(terminal);
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
    let mut viewport = EditorViewport::default();
    render_editor(&path, &text, cursor, &status, terminal, &mut viewport);

    loop {
        let event = keyboard::next_event_for(terminal).await;
        crate::terminal::set_output_terminal(terminal);
        let previous_cursor = cursor;
        let previous_viewport = (viewport.top_line, viewport.left_column);
        let update_cursor_only = status.is_empty()
            && matches!(
                event,
                keyboard::KeyEvent::Left
                    | keyboard::KeyEvent::Right
                    | keyboard::KeyEvent::Up
                    | keyboard::KeyEvent::Down
            );
        match event {
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
            keyboard::KeyEvent::Character('\t') => {
                if insert_editor_text(&mut text, &mut cursor, "\t") {
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
        if update_cursor_only {
            let (columns, rows) = crate::terminal::terminal_dimensions();
            let visible_rows = rows.saturating_sub(2).max(1);
            let (cursor_line, cursor_column) = editor_cursor_position(&text, cursor);
            update_editor_viewport(
                cursor_line,
                cursor_column,
                visible_rows,
                columns,
                &mut viewport,
            );
            if previous_viewport != (viewport.top_line, viewport.left_column) {
                render_editor(&path, &text, cursor, &status, terminal, &mut viewport);
            } else if previous_cursor != cursor {
                render_editor_caret(&text, previous_cursor, terminal, &viewport, false);
                render_editor_caret(&text, cursor, terminal, &viewport, true);
            }
        } else {
            render_editor(&path, &text, cursor, &status, terminal, &mut viewport);
        }
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

#[derive(Default)]
struct EditorViewport {
    top_line: usize,
    left_column: usize,
}

fn render_editor(
    path: &str,
    text: &str,
    cursor: usize,
    status: &str,
    terminal: usize,
    viewport: &mut EditorViewport,
) {
    let (columns, rows) = crate::terminal::terminal_dimensions();
    let footer_rows = if status.is_empty() { 1 } else { 2 };
    let visible_rows = rows.saturating_sub(1 + footer_rows).max(1);
    let (cursor_line, cursor_column) = editor_cursor_position(text, cursor);
    update_editor_viewport(cursor_line, cursor_column, visible_rows, columns, viewport);

    crate::terminal::terminal_clear_for(terminal);
    render_editor_text_at(terminal, 0, 0, "OCTAGRAM nano - ", columns);
    let prefix_columns = "OCTAGRAM nano - ".chars().count();
    render_editor_text_at(
        terminal,
        0,
        prefix_columns,
        path,
        columns.saturating_sub(prefix_columns),
    );

    let mut lines = text.split('\n').skip(viewport.top_line);
    for row in 0..visible_rows {
        if let Some(line) = lines.next() {
            render_editor_line(terminal, row + 1, line, viewport.left_column, columns);
        }
    }
    render_editor_caret(text, cursor, terminal, viewport, true);

    let help = "Ctrl+S Save   Ctrl+Q Quit   Tab Insert   Arrow keys move";
    let help_row = visible_rows + 1;
    if status.is_empty() {
        render_editor_text_at(terminal, help_row, 0, help, columns);
    } else {
        render_editor_text_at(terminal, help_row, 0, help, columns);
        render_editor_text_at(terminal, help_row + 1, 0, status, columns);
    }
}

fn editor_cursor_position(text: &str, cursor: usize) -> (usize, usize) {
    let before_cursor = &text[..cursor];
    let line = before_cursor.bytes().filter(|byte| *byte == b'\n').count();
    let line_start = before_cursor.rfind('\n').map_or(0, |index| index + 1);
    let column = before_cursor[line_start..].chars().count();
    (line, column)
}

fn update_editor_viewport(
    cursor_line: usize,
    cursor_column: usize,
    visible_rows: usize,
    visible_columns: usize,
    viewport: &mut EditorViewport,
) {
    let visible_rows = visible_rows.max(1);
    let visible_columns = visible_columns.max(1);
    if cursor_line < viewport.top_line {
        viewport.top_line = cursor_line;
    } else if cursor_line >= viewport.top_line.saturating_add(visible_rows) {
        viewport.top_line = cursor_line + 1 - visible_rows;
    }
    if cursor_column < viewport.left_column {
        viewport.left_column = cursor_column;
    } else if cursor_column >= viewport.left_column.saturating_add(visible_columns) {
        viewport.left_column = cursor_column + 1 - visible_columns;
    }
}

fn render_editor_line(
    terminal: usize,
    screen_row: usize,
    line: &str,
    left_column: usize,
    width: usize,
) {
    for (screen_column, character) in line.chars().skip(left_column).take(width).enumerate() {
        crate::terminal::terminal_put_char_at(
            terminal,
            screen_row,
            screen_column,
            editor_display_character(character),
        );
    }
}

fn render_editor_text_at(terminal: usize, row: usize, column: usize, text: &str, width: usize) {
    for (offset, character) in text.chars().take(width).enumerate() {
        crate::terminal::terminal_put_char_at(terminal, row, column + offset, character);
    }
}

fn render_editor_caret(
    text: &str,
    cursor: usize,
    terminal: usize,
    viewport: &EditorViewport,
    visible: bool,
) {
    let (line, column) = editor_cursor_position(text, cursor);
    let screen_row = line.saturating_sub(viewport.top_line) + 1;
    let screen_column = column.saturating_sub(viewport.left_column);
    let character = text
        .split('\n')
        .nth(line)
        .and_then(|line_text| line_text.chars().nth(column))
        .unwrap_or(' ');
    let character = editor_display_character(character);
    if visible {
        crate::terminal::terminal_put_char_at_highlighted(
            terminal,
            screen_row,
            screen_column,
            character,
        );
    } else {
        crate::terminal::terminal_put_char_at(terminal, screen_row, screen_column, character);
    }
}

fn editor_display_character(character: char) -> char {
    if character == '\t' { '>' } else { character }
}

fn insert_editor_text(text: &mut String, cursor: &mut usize, inserted: &str) -> bool {
    let Some(new_length) = text.len().checked_add(inserted.len()) else {
        return false;
    };
    if new_length > MAX_FILE_OPERATION || text.try_reserve(inserted.len()).is_err() {
        return false;
    }
    text.insert_str(*cursor, inserted);
    *cursor += inserted.len();
    true
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
    read_file_limited(vfs, path, MAX_FILE_OPERATION)
}

fn read_file_limited(vfs: &mut Vfs, path: &str, limit: usize) -> Result<Vec<u8>, FsError> {
    let metadata = vfs.metadata(path)?;
    if metadata.node_type != NodeType::File {
        return Err(FsError::IsDirectory);
    }
    if metadata.len > limit {
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
    if error == FsError::NoSpace {
        crate::println!(
            "{command}: {path}: NoSpace (file too large for this command, or the RAM filesystem/heap is full; mount a disk and work there)"
        );
        return;
    }
    crate::println!("{command}: {path}: {error:?}");
}

#[cfg(test)]
mod tests {
    use super::{
        EditorViewport, editor_cursor_position, editor_display_character, insert_editor_text,
        matching_lines, move_vertical, next_char_boundary, parse_grep_arguments,
        parse_ipv4_address, parse_line_selection_arguments, previous_char_boundary,
        program_candidates, resolve_path, selected_line_range, split_shell_words, text_counts,
        update_editor_viewport,
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
    fn selects_head_and_tail_lines_with_empty_and_oversized_counts() {
        assert_eq!(selected_line_range(4, 2, false), 0..2);
        assert_eq!(selected_line_range(4, 2, true), 2..4);
        assert_eq!(selected_line_range(4, 0, true), 4..4);
        assert_eq!(selected_line_range(4, 8, false), 0..4);
    }

    #[test]
    fn parses_line_counts_and_literal_grep_arguments() {
        let head_words = split_shell_words("-n 2 'two words.txt'").unwrap();
        assert_eq!(
            parse_line_selection_arguments(&head_words),
            Ok((2, "two words.txt"))
        );
        assert!(
            parse_line_selection_arguments(&split_shell_words("-n many file").unwrap()).is_err()
        );

        let grep_words = split_shell_words("-n 'two words' file.txt").unwrap();
        assert_eq!(
            parse_grep_arguments(&grep_words),
            Ok((true, "two words", "file.txt"))
        );
        assert!(parse_grep_arguments(&split_shell_words("pattern").unwrap()).is_err());
    }

    #[test]
    fn matches_literal_text_and_preserves_line_numbers() {
        assert_eq!(
            matching_lines("one\nneedle\nother needle\n", "needle").collect::<Vec<_>>(),
            vec![(1, "needle"), (2, "other needle")]
        );
    }

    #[test]
    fn counts_newlines_words_and_utf8_bytes() {
        assert_eq!(text_counts("é\none two"), (1, 3, 10));
        assert_eq!(text_counts(""), (0, 0, 0));
    }

    #[test]
    fn parses_only_four_valid_ipv4_octets() {
        assert_eq!(parse_ipv4_address("192.168.42.1"), Some([192, 168, 42, 1]));
        assert_eq!(parse_ipv4_address("192.168.42.256"), None);
        assert_eq!(parse_ipv4_address("192.168.42"), None);
        assert_eq!(parse_ipv4_address("192.168.42.1.extra"), None);
    }

    #[test]
    fn editor_cursor_moves_between_lines_and_utf8_characters() {
        let text = "aé\nxy";
        assert_eq!(previous_char_boundary(text, 3), Some(1));
        assert_eq!(next_char_boundary(text, 1), 3);
        assert_eq!(move_vertical(text, 3, true), 6);
        assert_eq!(move_vertical(text, 5, false), 1);
        assert_eq!(editor_cursor_position("alpha\néx", 8), (1, 1));
    }

    #[test]
    fn editor_tab_inserts_a_literal_tab_at_the_cursor() {
        let mut text = String::from("make");
        let mut cursor = 2;

        assert!(insert_editor_text(&mut text, &mut cursor, "\t"));
        assert_eq!(text, "ma\tke");
        assert_eq!(cursor, 3);
        assert_eq!(editor_display_character('\t'), '>');
    }

    #[test]
    fn editor_viewport_scrolls_only_when_cursor_reaches_an_edge() {
        let mut viewport = EditorViewport::default();
        update_editor_viewport(2, 4, 3, 5, &mut viewport);
        assert_eq!((viewport.top_line, viewport.left_column), (0, 0));

        update_editor_viewport(3, 5, 3, 5, &mut viewport);
        assert_eq!((viewport.top_line, viewport.left_column), (1, 1));

        update_editor_viewport(0, 0, 3, 5, &mut viewport);
        assert_eq!((viewport.top_line, viewport.left_column), (0, 0));
    }
}
