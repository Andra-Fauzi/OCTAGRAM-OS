#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use alloc::ffi::CString;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::alloc::Layout;
use core::ffi::{c_char, CStr};
use core::panic::PanicInfo;
use core::slice;
use octagram_runtime::{
    chdir, close, environ, execve, exit, fork, open, read, stat_path, waitpid, write_all,
    O_RDONLY, STDERR, STDOUT,
};

const MAX_MAKEFILE_SIZE: usize = 64 * 1024;
const MAX_RULES: usize = 128;

#[derive(Clone)]
struct Rule {
    target: String,
    dependencies: Vec<String>,
    recipes: Vec<String>,
    stem: Option<String>,
}

struct Makefile {
    rules: Vec<Rule>,
    phony_targets: Vec<String>,
    variables: Vec<(String, String)>,
    default_target: Option<String>,
}

#[derive(Clone, Copy)]
enum AssignmentKind {
    Replace,
    IfUnset,
    Append,
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8) -> i32 {
    match run(argc, argv) {
        Ok(status) => status,
        Err(error) => {
            report(&format!("make: {error}\n"));
            2
        }
    }
}

fn run(argc: i32, argv: *const *const u8) -> Result<i32, String> {
    let arguments = collect_arguments(argc, argv)?;
    let mut makefile_path = String::from("Makefile");
    let mut change_directory = None;
    let mut targets = Vec::new();
    let mut dry_run = false;
    let mut silent = false;
    let mut always_make = false;
    let mut overrides = Vec::new();
    let mut index = 0;

    while index < arguments.len() {
        match arguments[index].as_str() {
            "--help" | "-h" => {
                write_all(
                    STDOUT,
                    b"usage: make [-B] [-n] [-s] [-j1] [-C directory] [-f file] [NAME=value] [target...]\n\
Supported Makefile features: includes, explicit dependencies, single-stem pattern rules, \n\
variables, $@/$</$^/$*, .PHONY, and timestamp-based incremental rebuilds.\n\
Targets rebuild when missing or older than prerequisites; -B forces a rebuild.\n\
Shell operators, wildcards, and multi-stem pattern rules are unsupported.\n\
",
                )
                .map_err(|error| format!("write failed ({error})"))?;
                return Ok(0);
            }
            "-B" => always_make = true,
            "-n" | "--dry-run" => dry_run = true,
            "-s" | "--silent" => silent = true,
            "-j1" => {}
            "-f" | "-C" => {
                index += 1;
                let Some(value) = arguments.get(index) else {
                    return Err(format!("missing argument after {}", arguments[index - 1]));
                };
                if arguments[index - 1] == "-f" {
                    makefile_path = value.clone();
                } else {
                    change_directory = Some(value.clone());
                }
            }
            option if option.starts_with('-') => {
                return Err(format!("unsupported option: {option}"));
            }
            value if value.contains('=') && targets.is_empty() => {
                let (name, value) = value.split_once('=').unwrap();
                if !valid_variable_name(name) {
                    return Err(format!("invalid variable assignment: {name}"));
                }
                set_variable(&mut overrides, name, value);
            }
            target => targets.push(target.to_string()),
        }
        index += 1;
    }

    if let Some(directory) = change_directory {
        let directory =
            CString::new(directory).map_err(|_| String::from("directory contains a NUL byte"))?;
        chdir(&directory).map_err(|error| format!("cannot change directory ({error})"))?;
    }

    let mut include_stack = Vec::new();
    let mut total_size = 0usize;
    let text = load_makefile(&makefile_path, 0, &mut include_stack, &mut total_size)?;
    let mut makefile = parse_makefile(&text)?;
    for (name, value) in overrides {
        set_variable(&mut makefile.variables, &name, &value);
    }

    if targets.is_empty() {
        let Some(target) = makefile.default_target.clone() else {
            return Err("no targets in Makefile".into());
        };
        targets.push(target);
    }

    let mut built = Vec::new();
    let mut visiting = Vec::new();
    for target in targets {
        if let Err(error) = build_target(
            &target, &makefile, &mut visiting, &mut built, dry_run, silent, always_make,
        ) {
            report(&format!("make: {error}\n"));
            return Ok(2);
        }
    }
    Ok(0)
}

fn collect_arguments(argc: i32, argv: *const *const u8) -> Result<Vec<String>, String> {
    if argc < 1 || argv.is_null() {
        return Err("invalid argument vector".into());
    }
    let pointers = unsafe { slice::from_raw_parts(argv, argc as usize) };
    let mut arguments = Vec::new();
    for pointer in pointers.iter().skip(1) {
        if pointer.is_null() {
            return Err("invalid argument".into());
        }
        let argument = unsafe { CStr::from_ptr(*pointer as *const c_char) };
        arguments.push(
            argument
                .to_str()
                .map_err(|_| String::from("arguments must be UTF-8"))?
                .to_string(),
        );
    }
    Ok(arguments)
}

fn load_makefile(
    path: &str,
    depth: usize,
    include_stack: &mut Vec<String>,
    total_size: &mut usize,
) -> Result<String, String> {
    const MAX_INCLUDE_DEPTH: usize = 8;
    if depth > MAX_INCLUDE_DEPTH {
        return Err("Makefile include nesting is too deep".into());
    }
    if include_stack.iter().any(|included| included == path) {
        return Err(format!("recursive Makefile include: {path}"));
    }

    let source = read_file(path)?;
    *total_size = total_size.saturating_add(source.len());
    if *total_size > MAX_MAKEFILE_SIZE {
        return Err("combined Makefiles are too large".into());
    }
    let text = core::str::from_utf8(&source)
        .map_err(|_| format!("{path}: Makefile is not UTF-8"))?;
    include_stack.push(path.to_string());

    let mut expanded = String::new();
    for (line_number, line) in text.lines().enumerate() {
        // A recipe may invoke a program named "include"; only non-recipe
        // lines can contain Makefile include directives.
        let directive = if line.starts_with('\t') { "" } else { line.trim() };
        if let Some(includes) = directive.strip_prefix("include ") {
            let includes = includes.split('#').next().unwrap_or("").trim();
            if includes.is_empty() {
                include_stack.pop();
                return Err(format!("{path}:{}: empty include directive", line_number + 1));
            }
            for included_path in includes.split_whitespace() {
                let included = load_makefile(included_path, depth + 1, include_stack, total_size)
                    .map_err(|error| format!("{path}:{}: {error}", line_number + 1))?;
                expanded.push_str(&included);
                expanded.push('\n');
            }
        } else {
            expanded.push_str(line);
            expanded.push('\n');
        }
        if expanded.len() > MAX_MAKEFILE_SIZE {
            include_stack.pop();
            return Err("combined Makefiles are too large".into());
        }
    }

    include_stack.pop();
    Ok(expanded)
}

fn parse_makefile(source: &str) -> Result<Makefile, String> {
    if source.len() > MAX_MAKEFILE_SIZE {
        return Err("Makefile is too large".into());
    }
    let mut makefile = Makefile {
        rules: Vec::new(),
        phony_targets: Vec::new(),
        variables: alloc::vec![(String::from("CC"), String::from("tcc"))],
        default_target: None,
    };
    let mut active_rule: Option<usize> = None;

    for (line_number, original_line) in source.lines().enumerate() {
        let line = original_line.trim_end_matches('\r');
        if let Some(recipe) = line.strip_prefix('\t') {
            let Some(rule_index) = active_rule else {
                return Err(format!(
                    "Makefile:{}: recipe without a target",
                    line_number + 1
                ));
            };
            if !recipe.trim().is_empty() {
                makefile.rules[rule_index]
                    .recipes
                    .push(recipe.trim().to_string());
            }
            continue;
        }

        if active_rule.is_some() && line.starts_with(' ') && !line.trim().is_empty() {
            return Err(format!(
                "Makefile:{}: recipe must start with a tab, not spaces",
                line_number + 1
            ));
        }

        active_rule = None;
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some((name, value, kind)) = parse_assignment(line) {
            if !valid_variable_name(name) {
                return Err(format!(
                    "Makefile:{}: invalid variable name `{name}`",
                    line_number + 1
                ));
            }
            match kind {
                AssignmentKind::Replace => set_variable(&mut makefile.variables, name, value),
                AssignmentKind::IfUnset => {
                    if get_variable(&makefile.variables, name).is_none() {
                        set_variable(&mut makefile.variables, name, value);
                    }
                }
                AssignmentKind::Append => {
                    let old = get_variable(&makefile.variables, name).unwrap_or("");
                    let value = if old.is_empty() {
                        value.to_string()
                    } else {
                        format!("{old} {value}")
                    };
                    set_variable(&mut makefile.variables, name, &value);
                }
            }
            continue;
        }

        let Some((target_text, dependency_text)) = line.split_once(':') else {
            return Err(format!(
                "Makefile:{}: expected a target or variable",
                line_number + 1
            ));
        };
        let target_text = expand(target_text.trim(), &makefile.variables, None, 0)?;
        let mut target_words = target_text.split_whitespace();
        let Some(target) = target_words.next() else {
            return Err(format!("Makefile:{}: missing target", line_number + 1));
        };
        if target_words.next().is_some() {
            return Err(format!(
                "Makefile:{}: multiple targets per rule are unsupported",
                line_number + 1
            ));
        }
        let dependencies = expand(dependency_text.trim(), &makefile.variables, None, 0)?
            .split_whitespace()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if target == ".PHONY" {
            for target in dependencies {
                if !makefile.phony_targets.contains(&target) {
                    makefile.phony_targets.push(target);
                }
            }
            continue;
        }

        let rule_index =
            if let Some(index) = makefile.rules.iter().position(|rule| rule.target == target) {
                for dependency in dependencies {
                    if !makefile.rules[index].dependencies.contains(&dependency) {
                        makefile.rules[index].dependencies.push(dependency);
                    }
                }
                index
            } else {
                if makefile.rules.len() >= MAX_RULES {
                    return Err("too many targets in Makefile".into());
                }
                if makefile.default_target.is_none() && !target.starts_with('.') && !target.contains('%') {
                    makefile.default_target = Some(target.to_string());
                }
                makefile.rules.push(Rule {
                    target: target.to_string(),
                    dependencies,
                    recipes: Vec::new(),
                    stem: None,
                });
                makefile.rules.len() - 1
            };
        active_rule = Some(rule_index);
    }

    Ok(makefile)
}

fn parse_assignment(line: &str) -> Option<(&str, &str, AssignmentKind)> {
    let equals = line.find('=')?;
    let before = line[..equals].trim_end();
    let (name, kind) = if let Some(name) = before.strip_suffix('+') {
        (name.trim(), AssignmentKind::Append)
    } else if let Some(name) = before.strip_suffix('?') {
        (name.trim(), AssignmentKind::IfUnset)
    } else if let Some(name) = before.strip_suffix(':') {
        (name.trim(), AssignmentKind::Replace)
    } else {
        (before.trim(), AssignmentKind::Replace)
    };
    if name.is_empty() || name.contains(char::is_whitespace) {
        return None;
    }
    Some((name, line[equals + 1..].trim(), kind))
}

fn valid_variable_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn set_variable(variables: &mut Vec<(String, String)>, name: &str, value: &str) {
    if let Some((_, current)) = variables.iter_mut().find(|(key, _)| key == name) {
        *current = value.to_string();
    } else {
        variables.push((name.to_string(), value.to_string()));
    }
}

fn get_variable<'a>(variables: &'a [(String, String)], name: &str) -> Option<&'a str> {
    variables
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn expand(
    source: &str,
    variables: &[(String, String)],
    automatic: Option<(&str, &str, &str, &str)>,
    depth: usize,
) -> Result<String, String> {
    if depth > 16 {
        return Err("recursive variable expansion".into());
    }
    let mut output = String::new();
    let mut chars = source.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '$' {
            output.push(character);
            continue;
        }
        let Some(next) = chars.next() else {
            output.push('$');
            break;
        };
        let value = match next {
            '$' => {
                output.push('$');
                continue;
            }
            '@' => automatic.map(|(target, _, _, _)| target).unwrap_or(""),
            '<' => automatic.map(|(_, first, _, _)| first).unwrap_or(""),
            '^' => automatic.map(|(_, _, all, _)| all).unwrap_or(""),
            '*' => automatic.map(|(_, _, _, stem)| stem).unwrap_or(""),
            '(' | '{' => {
                let closing = if next == '(' { ')' } else { '}' };
                let mut name = String::new();
                let mut found = false;
                for part in chars.by_ref() {
                    if part == closing {
                        found = true;
                        break;
                    }
                    name.push(part);
                }
                if !found {
                    return Err("unterminated variable reference".into());
                }
                let raw = get_variable(variables, &name).unwrap_or("");
                output.push_str(&expand(raw, variables, automatic, depth + 1)?);
                continue;
            }
            other => {
                output.push(other);
                continue;
            }
        };
        output.push_str(value);
    }
    Ok(output)
}

fn instantiate_pattern_rule(rule: &Rule, target: &str) -> Option<Rule> {
    let (prefix, suffix) = rule.target.split_once('%')?;
    if !target.starts_with(prefix) || !target.ends_with(suffix) {
        return None;
    }
    let stem_end = target.len().checked_sub(suffix.len())?;
    if stem_end < prefix.len() {
        return None;
    }
    let stem = &target[prefix.len()..stem_end];
    let substitute = |value: &str| value.replace('%', stem);
    Some(Rule {
        target: target.to_string(),
        dependencies: rule
            .dependencies
            .iter()
            .map(|dependency| substitute(dependency))
            .collect(),
        recipes: rule.recipes.clone(),
        stem: Some(stem.to_string()),
    })
}

fn build_target(
    target: &str,
    makefile: &Makefile,
    visiting: &mut Vec<String>,
    built: &mut Vec<String>,
    dry_run: bool,
    silent: bool,
    always_make: bool,
) -> Result<(), String> {
    if built.iter().any(|name| name == target) {
        return Ok(());
    }
    if visiting.iter().any(|name| name == target) {
        return Err(format!("dependency cycle involving `{target}`"));
    }
    let is_phony = makefile.phony_targets.iter().any(|name| name == target);
    let rule = makefile
        .rules
        .iter()
        .find(|rule| rule.target == target)
        .cloned()
        .or_else(|| {
            makefile
                .rules
                .iter()
                .find_map(|rule| instantiate_pattern_rule(rule, target))
        });
    let Some(rule) = rule else {
        return if is_phony || file_exists(target) {
            Ok(())
        } else {
            Err(format!("no rule to make target `{target}`"))
        };
    };
    visiting.push(target.to_string());
    for dependency in &rule.dependencies {
        build_target(
            dependency,
            makefile,
            visiting,
            built,
            dry_run,
            silent,
            always_make,
        )?;
    }
    visiting.pop();

    // Aggregate targets without recipes (for example, "all") are graph nodes,
    // not output files. Their prerequisites have already been visited.
    if rule.recipes.is_empty() {
        built.push(target.to_string());
        return Ok(());
    }

    let target_mtime = if is_phony { None } else { file_mtime(target) };
    let prerequisite_mtimes = rule
        .dependencies
        .iter()
        .map(|dependency| file_mtime(dependency))
        .collect::<Vec<_>>();
    if !needs_rebuild(target_mtime, &prerequisite_mtimes, always_make) {
        built.push(target.to_string());
        return Ok(());
    }

    let first = rule.dependencies.first().map(String::as_str).unwrap_or("");
    let all = rule.dependencies.join(" ");
    for recipe in &rule.recipes {
        run_recipe(
            recipe,
            &rule.target,
            first,
            &all,
            &makefile.variables,
            rule.stem.as_deref().unwrap_or(""),
            dry_run,
            silent,
        )?;
    }
    built.push(target.to_string());
    Ok(())
}

fn run_recipe(
    recipe: &str,
    target: &str,
    first_dependency: &str,
    all_dependencies: &str,
    variables: &[(String, String)],
    stem: &str,
    dry_run: bool,
    globally_silent: bool,
) -> Result<(), String> {
    let mut recipe = recipe.trim();
    let mut silent = false;
    let mut ignore_errors = false;
    while let Some(prefix) = recipe.chars().next() {
        match prefix {
            '@' => {
                silent = true;
                recipe = recipe[1..].trim_start();
            }
            '-' => {
                ignore_errors = true;
                recipe = recipe[1..].trim_start();
            }
            _ => break,
        }
    }
    let expanded = expand(
        recipe,
        variables,
        Some((target, first_dependency, all_dependencies, stem)),
        0,
    )?;
    let arguments = split_words(&expanded)?;
    if arguments.is_empty() {
        return Ok(());
    }
    if expanded
        .chars()
        .any(|character| matches!(character, ';' | '|' | '&' | '<' | '>'))
    {
        return Err(String::from(
            "shell operators are unsupported in recipes; invoke one program per recipe",
        ));
    }
    if !silent && !globally_silent {
        write_all(STDOUT, expanded.as_bytes())
            .and_then(|_| write_all(STDOUT, b"\n"))
            .map_err(|error| format!("write failed ({error})"))?;
    }

    if dry_run {
        return Ok(());
    }

    let command = &arguments[0];
    let executable = if command.contains('/') {
        command.clone()
    } else {
        format!("/bin/{command}")
    };
    let executable =
        CString::new(executable).map_err(|_| String::from("command path contains a NUL byte"))?;
    let c_arguments = arguments
        .iter()
        .map(|argument| {
            CString::new(argument.as_str())
                .map_err(|_| String::from("command argument contains a NUL byte"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let pointers = c_arguments
        .iter()
        .map(|argument| argument.as_ptr())
        .chain(core::iter::once(core::ptr::null()))
        .collect::<Vec<_>>();
    let mut environment = Vec::new();
    unsafe {
        let mut entry = environ;
        while !entry.is_null() && !(*entry).is_null() {
            environment.push(*entry as *const c_char);
            entry = entry.add(1);
        }
    }
    environment.push(core::ptr::null());

    let pid = fork().map_err(|error| format!("cannot start {} ({error})", arguments[0]))?;
    if pid == 0 {
        if let Err(error) = execve(&executable, &pointers, &environment) {
            report(&format!("make: cannot start {} ({error})\n", arguments[0]));
            exit(127);
        }
        exit(127);
    }
    let mut status = 0;
    waitpid(pid, Some(&mut status)).map_err(|error| format!("waitpid failed ({error})"))?;
    if status != 0 && !ignore_errors {
        return Err(format!("recipe failed with status {status}"));
    }
    Ok(())
}

fn split_words(source: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut started = false;
    for character in source.chars() {
        if escaped {
            word.push(character);
            escaped = false;
            started = true;
        } else if character == '\\' && quote != Some('\'') {
            escaped = true;
            started = true;
        } else if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            } else {
                word.push(character);
            }
        } else {
            match character {
                '\'' | '"' => {
                    quote = Some(character);
                    started = true;
                }
                character if character.is_whitespace() => {
                    if started {
                        words.push(core::mem::take(&mut word));
                        started = false;
                    }
                }
                _ => {
                    word.push(character);
                    started = true;
                }
            }
        }
    }
    if escaped || quote.is_some() {
        return Err("unmatched quote or escape in recipe".into());
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

fn file_exists(path: &str) -> bool {
    let Ok(path) = CString::new(path) else {
        return false;
    };
    stat_path(&path).is_ok()
}

/// Return EXT2/VFS mtime when it is known. Zero is the ABI's "unknown" value.
fn file_mtime(path: &str) -> Option<u64> {
    let path = CString::new(path).ok()?;
    let metadata = stat_path(&path).ok()?;
    (metadata.modified_seconds != 0).then_some(u64::from(metadata.modified_seconds))
}

/// Decide whether a target must be rebuilt. Unknown dependency times fail
/// conservatively: rebuild rather than accidentally reusing stale output.
fn needs_rebuild(
    target_mtime: Option<u64>,
    prerequisite_mtimes: &[Option<u64>],
    always_make: bool,
) -> bool {
    if always_make {
        return true;
    }
    let Some(target_mtime) = target_mtime else {
        return true;
    };
    prerequisite_mtimes
        .iter()
        .any(|modified| modified.is_none_or(|modified| modified > target_mtime))
}

fn read_file(path: &str) -> Result<Vec<u8>, String> {
    let c_path = CString::new(path).map_err(|_| String::from("Makefile path contains a NUL"))?;
    let descriptor =
        open(&c_path, O_RDONLY).map_err(|error| format!("cannot open {path} ({error})"))?;
    let mut contents = Vec::new();
    let mut buffer = [0u8; 1024];
    let read_result = loop {
        let count = match read(descriptor, &mut buffer) {
            Ok(count) => count,
            Err(error) => break Err(format!("cannot read {path} ({error})")),
        };
        if count == 0 {
            break Ok(());
        }
        if contents.len().saturating_add(count) > MAX_MAKEFILE_SIZE {
            break Err(String::from("Makefile is too large"));
        }
        contents.extend_from_slice(&buffer[..count]);
    };
    let close_result = close(descriptor);
    read_result?;
    close_result.map_err(|error| format!("close failed ({error})"))?;
    Ok(contents)
}

fn report(message: &str) {
    let _ = write_all(STDERR, message.as_bytes());
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    report("make: compiler panicked\n");
    exit(101)
}

#[alloc_error_handler]
fn allocation_error(_layout: Layout) -> ! {
    report("make: out of memory\n");
    exit(127)
}
