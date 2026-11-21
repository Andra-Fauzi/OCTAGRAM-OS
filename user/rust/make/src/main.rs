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
    O_CREAT, O_RDONLY, O_TRUNC, O_WRONLY, STDERR, STDOUT,
};

const MAX_MAKEFILE_SIZE: usize = 64 * 1024;
const MAX_RULES: usize = 128;

#[derive(Clone)]
struct BuildRecord {
    target: String,
    status: String,
    modified_seconds: Option<u64>,
    size_bytes: Option<u64>,
    sha256: Option<String>,
    dependencies: String,
}

#[derive(Clone)]
struct Rule {
    target: String,
    dependencies: Vec<String>,
    order_only_dependencies: Vec<String>,
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
    let mut print_database = false;
    let mut manifest_path = None;
    let mut overrides = Vec::new();
    let mut index = 0;

    while index < arguments.len() {
        match arguments[index].as_str() {
            "--help" | "-h" => {
                write_all(
                    STDOUT,
                    b"usage: make [-B] [-n] [-s] [-p] [-j1] [-C directory] [-f file] [--manifest file] [NAME=value] [target...]\n\
-p/--print-database prints variables and rules without building.\n\
Supported Makefile features: includes, explicit dependencies, single-stem pattern rules, \n\
variables, $@/$</$^/$*, .PHONY, and timestamp-based incremental rebuilds.\n\
Targets rebuild when missing or older than prerequisites; -B forces a rebuild.\n\
Recipe lines may chain commands with ';' or '&&'; other shell operators,\n\
wildcards, and multi-stem pattern rules are unsupported.\n",
                )
                .map_err(|error| format!("write failed ({error})"))?;
                return Ok(0);
            }
            "-B" => always_make = true,
            "-p" | "--print-database" => print_database = true,
            "-n" | "--dry-run" => dry_run = true,
            "-s" | "--silent" => silent = true,
            "-j1" => {}
            "--manifest" => {
                index += 1;
                let Some(value) = arguments.get(index) else {
                    return Err(String::from("missing argument after --manifest"));
                };
                manifest_path = Some(value.clone());
            }
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

    if print_database {
        return print_make_database(&makefile);
    }

    if targets.is_empty() {
        let Some(target) = makefile.default_target.clone() else {
            return Err("no targets in Makefile".into());
        };
        targets.push(target);
    }

    let mut built = Vec::new();
    let mut visiting = Vec::new();
    let mut records = Vec::new();
    for target in targets {
        if let Err(error) = build_target(
            &target, &makefile, &mut visiting, &mut built, &mut records,
            dry_run, silent, always_make,
        ) {
            report(&format!("make: {error}\n"));
            return Ok(2);
        }
    }
    if let Some(path) = manifest_path {
        if let Err(error) = write_build_manifest(&path, &records) {
            report(&format!("make: {error}\n"));
            return Ok(2);
        }
    }
    Ok(0)
}

/// Print the parsed variable and rule database without building anything, so a
/// build graph can be inspected or diffed inside OCTAGRAM.
fn print_make_database(makefile: &Makefile) -> Result<i32, String> {
    let mut output = String::from("# variables\n");
    for (name, value) in &makefile.variables {
        output.push_str(&format!("{name} = {value}\n"));
    }
    output.push_str("# rules\n");
    for rule in &makefile.rules {
        output.push_str(&rule.target);
        output.push(':');
        for dependency in &rule.dependencies {
            output.push(' ');
            output.push_str(dependency);
        }
        if !rule.order_only_dependencies.is_empty() {
            output.push_str(" |");
            for dependency in &rule.order_only_dependencies {
                output.push(' ');
                output.push_str(dependency);
            }
        }
        output.push('\n');
        for recipe in &rule.recipes {
            output.push_str(&format!("\t{recipe}\n"));
        }
    }
    if !makefile.phony_targets.is_empty() {
        output.push_str("# phony\n.PHONY:");
        for target in &makefile.phony_targets {
            output.push(' ');
            output.push_str(target);
        }
        output.push('\n');
    }
    write_all(STDOUT, output.as_bytes()).map_err(|error| format!("write failed ({error})"))?;
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
        let (normal_text, order_only_text) = dependency_text
            .split_once('|')
            .map(|(normal, order_only)| (normal, order_only))
            .unwrap_or((dependency_text, ""));
        let dependencies = expand(normal_text.trim(), &makefile.variables, None, 0)?
            .split_whitespace()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let order_only_dependencies = expand(order_only_text.trim(), &makefile.variables, None, 0)?
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
                for dependency in order_only_dependencies.iter() {
                    if !makefile.rules[index].order_only_dependencies.contains(dependency) {
                        makefile.rules[index].order_only_dependencies.push(dependency.clone());
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
                    order_only_dependencies,
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
    automatic: Option<(&str, &str, &str, &str, &str, &str)>,
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
            '@' => automatic.map(|(target, _, _, _, _, _)| target).unwrap_or(""),
            '<' => automatic.map(|(_, first, _, _, _, _)| first).unwrap_or(""),
            '^' => automatic.map(|(_, _, all, _, _, _)| all).unwrap_or(""),
            '*' => automatic.map(|(_, _, _, stem, _, _)| stem).unwrap_or(""),
            '?' => automatic.map(|(_, _, _, _, newer, _)| newer).unwrap_or(""),
            '|' => automatic.map(|(_, _, _, _, _, order_only)| order_only).unwrap_or(""),
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
        order_only_dependencies: rule
            .order_only_dependencies
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
    records: &mut Vec<BuildRecord>,
    dry_run: bool,
    silent: bool,
    always_make: bool,
) -> Result<(), String> {
    if built.iter().any(|name| name == target) {
        return Ok(());
    }
    if visiting.iter().any(|name| name == target) {
        return Err(format!("dependency cycle involving {target}"));
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
        if is_phony || file_exists(target) {
            records.push(BuildRecord {
                target: target.to_string(),
                status: if is_phony { "phony" } else { "source" }.to_string(),
                modified_seconds: file_mtime(target),
                size_bytes: file_size(target),
                sha256: file_sha256(target),
                dependencies: String::new(),
            });
            return Ok(());
        }
        return Err(format!("no rule to make target {target}"));
    };
    visiting.push(target.to_string());
    for dependency in rule.dependencies.iter().chain(&rule.order_only_dependencies) {
        build_target(
            dependency, makefile, visiting, built, records,
            dry_run, silent, always_make,
        )?;
    }
    visiting.pop();

    let mut dependencies = rule.dependencies.join(" ");
    if !rule.order_only_dependencies.is_empty() {
        dependencies.push_str(" | ");
        dependencies.push_str(&rule.order_only_dependencies.join(" "));
    }

    // Aggregate targets without recipes are graph nodes, not output files.
    if rule.recipes.is_empty() {
        records.push(BuildRecord {
            target: target.to_string(),
            status: "aggregate".to_string(),
            modified_seconds: file_mtime(target),
            size_bytes: file_size(target),
            sha256: file_sha256(target),
            dependencies,
        });
        built.push(target.to_string());
        return Ok(());
    }

    let target_mtime = if is_phony { None } else { file_mtime(target) };
    let prerequisite_mtimes = rule.dependencies
        .iter()
        .map(|dependency| file_mtime(dependency))
        .collect::<Vec<_>>();
    if !needs_rebuild(target_mtime, &prerequisite_mtimes, always_make) {
        records.push(BuildRecord {
            target: target.to_string(),
            status: "up-to-date".to_string(),
            modified_seconds: target_mtime,
            size_bytes: file_size(target),
            sha256: file_sha256(target),
            dependencies,
        });
        built.push(target.to_string());
        return Ok(());
    }

    let first = rule.dependencies.first().map(String::as_str).unwrap_or("");
    let all = rule.dependencies.join(" ");
    let newer = rule.dependencies.iter()
        .zip(&prerequisite_mtimes)
        .filter(|(_, modified)| {
            target_mtime.is_none_or(|target_time| modified.is_none_or(|time| time > target_time))
        })
        .map(|(dependency, _)| dependency.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let order_only = rule.order_only_dependencies.join(" ");
    for recipe in &rule.recipes {
        run_recipe(
            recipe, &rule.target, first, &all, &newer, &order_only,
            &makefile.variables, rule.stem.as_deref().unwrap_or(""),
            dry_run, silent,
        )?;
    }
    records.push(BuildRecord {
        target: target.to_string(),
        status: if dry_run { "would-build" } else { "built" }.to_string(),
        modified_seconds: if dry_run { target_mtime } else { file_mtime(target) },
        size_bytes: file_size(target),
        sha256: file_sha256(target),
        dependencies,
    });
    built.push(target.to_string());
    Ok(())
}

fn write_build_manifest(path: &str, records: &[BuildRecord]) -> Result<(), String> {
    let path = CString::new(path)
        .map_err(|_| String::from("manifest path contains a NUL byte"))?;
    let descriptor = open(&path, O_WRONLY | O_CREAT | O_TRUNC)
        .map_err(|error| format!("cannot create build manifest ({error})"))?;
    let mut output = String::from(
        "OCTAGRAM-BUILD-MANIFEST\t2\n# target\tstatus\tmtime_unix\tsize_bytes\tsha256\tdependencies\n",
    );
    for record in records {
        let modified = record.modified_seconds
            .map(|value| value.to_string())
            .unwrap_or_else(|| String::from("unknown"));
        let size = record.size_bytes
            .map(|value| value.to_string())
            .unwrap_or_else(|| String::from("unknown"));
        let digest = record.sha256.as_deref().unwrap_or("unknown");
        output.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            record.target, record.status, modified, size, digest, record.dependencies
        ));
    }
    let result = write_all(descriptor, output.as_bytes())
        .map_err(|error| format!("cannot write build manifest ({error})"));
    let close_result = close(descriptor)
        .map_err(|error| format!("cannot close build manifest ({error})"));
    result?;
    close_result?;
    Ok(())
}

fn run_recipe(
    recipe: &str,
    target: &str,
    first_dependency: &str,
    all_dependencies: &str,
    newer_dependencies: &str,
    order_only_dependencies: &str,
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
        Some((target, first_dependency, all_dependencies, stem, newer_dependencies, order_only_dependencies)),
        0,
    )?;
    let commands = split_recipe_commands(&expanded)?;
    for (index, (command_text, stop_on_failure)) in commands.iter().enumerate() {
        if !silent && !globally_silent {
            write_all(STDOUT, command_text.as_bytes())
                .and_then(|_| write_all(STDOUT, b"\n"))
                .map_err(|error| format!("write failed ({error})"))?;
        }
        if dry_run {
            continue;
        }
        let arguments = split_words(command_text)?;
        if arguments.is_empty() {
            continue;
        }
        let status = run_recipe_program(&arguments)?;
        if status != 0 {
            if ignore_errors || !stop_on_failure {
                continue;
            }
            return Err(format!(
                "recipe command {} failed with status {status}",
                index + 1
            ));
        }
    }
    Ok(())
}

/// Split a recipe line into commands joined by `;` (always continue) and `&&`
/// (stop on the first failure). Pipelines and redirection stay unsupported so a
/// recipe never silently runs something the shell would interpret differently.
fn split_recipe_commands(source: &str) -> Result<Vec<(String, bool)>, String> {
    let mut commands = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut stop_on_failure = false;
    let mut characters = source.chars().peekable();
    while let Some(character) = characters.next() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            current.push(character);
            escaped = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            }
            current.push(character);
            continue;
        }
        match character {
            '\'' | '"' => {
                quote = Some(character);
                current.push(character);
            }
            ';' => {
                push_recipe_command(&mut commands, &mut current, stop_on_failure)?;
                stop_on_failure = false;
            }
            '&' if characters.peek() == Some(&'&') => {
                characters.next();
                push_recipe_command(&mut commands, &mut current, stop_on_failure)?;
                stop_on_failure = true;
            }
            '|' | '>' | '<' | '&' | '`' => {
                return Err(String::from(
                    "unsupported shell operator in recipe; use one program per recipe or ';' / '&&'",
                ));
            }
            _ => current.push(character),
        }
    }
    if quote.is_some() || escaped {
        return Err(String::from("unmatched quote or escape in recipe"));
    }
    push_recipe_command(&mut commands, &mut current, stop_on_failure)?;
    Ok(commands)
}

fn push_recipe_command(
    commands: &mut Vec<(String, bool)>,
    current: &mut String,
    stop_on_failure: bool,
) -> Result<(), String> {
    let command = current.trim().to_string();
    current.clear();
    if command.is_empty() {
        return Err(String::from("empty command in recipe"));
    }
    commands.push((command, stop_on_failure));
    Ok(())
}

fn run_recipe_program(arguments: &[String]) -> Result<i32, String> {
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
    Ok(i32::try_from(status).unwrap_or(-1))
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

/// Streaming SHA-256 so manifest generation does not need to load an entire
/// build artifact into memory.
struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    total_len: u64,
}

impl Sha256 {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
        0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, mut input: &[u8]) {
        self.total_len = self.total_len.wrapping_add(input.len() as u64);
        if self.buffer_len != 0 {
            let count = core::cmp::min(64 - self.buffer_len, input.len());
            self.buffer[self.buffer_len..self.buffer_len + count]
                .copy_from_slice(&input[..count]);
            self.buffer_len += count;
            input = &input[count..];
            if self.buffer_len == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffer_len = 0;
            }
        }
        while input.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&input[..64]);
            self.compress(&block);
            input = &input[64..];
        }
        if !input.is_empty() {
            self.buffer[..input.len()].copy_from_slice(input);
            self.buffer_len = input.len();
        }
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut words = [0u32; 64];
        for index in 0..16 {
            let offset = index * 4;
            words[index] = u32::from_be_bytes([
                block[offset], block[offset + 1], block[offset + 2], block[offset + 3],
            ]);
        }
        for index in 16..64 {
            let x = words[index - 15];
            let y = words[index - 2];
            let s0 = x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3);
            let s1 = y.rotate_right(17) ^ y.rotate_right(19) ^ (y >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for index in 0..64 {
            let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(sum1)
                .wrapping_add(choice)
                .wrapping_add(Self::K[index])
                .wrapping_add(words[index]);
            let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = sum0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }

    fn finish(mut self) -> String {
        let bit_len = self.total_len.wrapping_mul(8);
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;
        if self.buffer_len > 56 {
            self.buffer[self.buffer_len..].fill(0);
            let block = self.buffer;
            self.compress(&block);
            self.buffer = [0; 64];
            self.buffer_len = 0;
        }
        self.buffer[self.buffer_len..56].fill(0);
        self.buffer[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.buffer;
        self.compress(&block);

        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut digest = String::with_capacity(64);
        for word in self.state {
            for byte in word.to_be_bytes() {
                digest.push(HEX[(byte >> 4) as usize] as char);
                digest.push(HEX[(byte & 0x0f) as usize] as char);
            }
        }
        digest
    }
}

fn file_sha256(path: &str) -> Option<String> {
    let c_path = CString::new(path).ok()?;
    let descriptor = open(&c_path, O_RDONLY).ok()?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 1024];
    let read_result = loop {
        match read(descriptor, &mut buffer) {
            Ok(0) => break true,
            Ok(count) => hash.update(&buffer[..count]),
            Err(_) => break false,
        }
    };
    let close_result = close(descriptor).is_ok();
    if read_result && close_result {
        Some(hash.finish())
    } else {
        None
    }
}

fn file_size(path: &str) -> Option<u64> {
    let path = CString::new(path).ok()?;
    stat_path(&path).ok().map(|metadata| metadata.size)
}

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
