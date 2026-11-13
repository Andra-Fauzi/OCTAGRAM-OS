//! Small native assembler, linker, and C-subset compiler for OCTAGRAM.

use crate::fs::{self, FsError};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

const OBJECT_MAGIC: &[u8; 8] = b"OCTOBJ1\0";
const MAX_SOURCE_SIZE: usize = 8 * 1024;
const MAX_C_EXPRESSION_DEPTH: usize = 64;
const MAX_C_EXPRESSION_OPERATIONS: usize = 64;
const MAX_C_CONTROL_DEPTH: usize = 32;
const MAX_INCLUDE_DEPTH: usize = 8;
const MAX_INCLUDE_FILES: usize = 32;
const MAX_INCLUDE_BYTES: usize = 32 * 1024;
const MAX_CODE_SIZE: usize = 64 * 1024;
const ELF_HEADER_SIZE: usize = 64;
const PROGRAM_HEADER_SIZE: usize = 56;
const ELF_LOAD_OFFSET: usize = 0x1000;
const ELF_BASE: u64 = 0x400000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolchainError {
    SourceTooLarge,
    InvalidUtf8,
    InvalidAssembly,
    InvalidObject,
    InvalidCSource,
    UnsupportedC,
    UnsupportedPreprocessor,
    IncludeNotFound,
    IncludeDepthExceeded,
    IncludeLimitExceeded,
    Include(FsError),
    OutOfBounds,
}

#[derive(Debug)]
enum Item {
    Label(String),
    Bytes(Vec<u8>),
    MoveImmediate {
        register: u8,
        value: u64,
    },
    MoveRegister {
        source: u8,
        destination: u8,
    },
    PushRegister(u8),
    PopRegister(u8),
    BinaryRegister {
        operation: BinaryOperation,
        source: u8,
        destination: u8,
    },
    TestRegister {
        left: u8,
        right: u8,
    },
    LeaRip {
        label: String,
        destination: u8,
    },
    RelativeBranch {
        label: String,
        call: bool,
    },
    ConditionalBranch {
        label: String,
        condition: BranchCondition,
    },
    Syscall,
    Return,
    Nop,
}

#[derive(Debug, Clone, Copy)]
enum BinaryOperation {
    Add,
    Subtract,
    Multiply,
}

#[derive(Debug, Clone, Copy)]
enum BranchCondition {
    Equal,
    NotEqual,
}

impl Item {
    fn encoded_size(&self) -> usize {
        match self {
            Self::Label(_) => 0,
            Self::Bytes(bytes) => bytes.len(),
            Self::MoveImmediate { .. } => 10,
            Self::MoveRegister { .. } => 3,
            Self::PushRegister(_) | Self::PopRegister(_) => 1,
            Self::BinaryRegister {
                operation: BinaryOperation::Multiply,
                ..
            } => 4,
            Self::BinaryRegister { .. } => 3,
            Self::TestRegister { .. } => 3,
            Self::LeaRip { .. } => 7,
            Self::RelativeBranch { .. } => 5,
            Self::ConditionalBranch { .. } => 6,
            Self::Syscall => 2,
            Self::Return | Self::Nop => 1,
        }
    }
}

pub fn assemble(source: &[u8]) -> Result<Vec<u8>, ToolchainError> {
    if source.len() > MAX_SOURCE_SIZE {
        return Err(ToolchainError::SourceTooLarge);
    }
    let source = core::str::from_utf8(source).map_err(|_| ToolchainError::InvalidUtf8)?;
    let items = parse_assembly(source)?;
    let mut labels = BTreeMap::new();
    let mut offsets = Vec::with_capacity(items.len());
    let mut code_size = 0usize;

    for item in &items {
        offsets.push(code_size);
        match item {
            Item::Label(name) => {
                if labels.insert(name.clone(), code_size).is_some() {
                    return Err(ToolchainError::InvalidAssembly);
                }
            }
            _ => {
                code_size = code_size
                    .checked_add(item.encoded_size())
                    .ok_or(ToolchainError::OutOfBounds)?;
                if code_size > MAX_CODE_SIZE {
                    return Err(ToolchainError::SourceTooLarge);
                }
            }
        }
    }

    let entry = *labels
        .get("_start")
        .ok_or(ToolchainError::InvalidAssembly)?;
    let mut code = Vec::with_capacity(code_size);
    for (item, offset) in items.iter().zip(offsets) {
        match item {
            Item::Label(_) => {}
            Item::Bytes(bytes) => code.extend_from_slice(bytes),
            Item::MoveImmediate { register, value } => {
                code.extend_from_slice(&[0x48, 0xB8 + register]);
                code.extend_from_slice(&value.to_le_bytes());
            }
            Item::MoveRegister {
                source,
                destination,
            } => {
                code.extend_from_slice(&[0x48, 0x89, 0xC0 | (source << 3) | destination]);
            }
            Item::PushRegister(register) => code.push(0x50 + register),
            Item::PopRegister(register) => code.push(0x58 + register),
            Item::BinaryRegister {
                operation,
                source,
                destination,
            } => match operation {
                BinaryOperation::Add => {
                    code.extend_from_slice(&[0x48, 0x01, 0xC0 | (source << 3) | destination]);
                }
                BinaryOperation::Subtract => {
                    code.extend_from_slice(&[0x48, 0x29, 0xC0 | (source << 3) | destination]);
                }
                BinaryOperation::Multiply => {
                    code.extend_from_slice(&[0x48, 0x0F, 0xAF, 0xC0 | (destination << 3) | source]);
                }
            },
            Item::TestRegister { left, right } => {
                code.extend_from_slice(&[0x48, 0x85, 0xC0 | (left << 3) | right]);
            }
            Item::LeaRip { label, destination } => {
                let target = *labels.get(label).ok_or(ToolchainError::InvalidAssembly)?;
                let displacement = relative_displacement(target, offset + 7)?;
                code.extend_from_slice(&[0x48, 0x8D, 0x05 | (destination << 3)]);
                code.extend_from_slice(&displacement.to_le_bytes());
            }
            Item::RelativeBranch { label, call } => {
                let target = *labels.get(label).ok_or(ToolchainError::InvalidAssembly)?;
                let displacement = relative_displacement(target, offset + 5)?;
                code.push(if *call { 0xE8 } else { 0xE9 });
                code.extend_from_slice(&displacement.to_le_bytes());
            }
            Item::ConditionalBranch { label, condition } => {
                let target = *labels.get(label).ok_or(ToolchainError::InvalidAssembly)?;
                let displacement = relative_displacement(target, offset + 6)?;
                code.extend_from_slice(&[
                    0x0F,
                    match condition {
                        BranchCondition::Equal => 0x84,
                        BranchCondition::NotEqual => 0x85,
                    },
                ]);
                code.extend_from_slice(&displacement.to_le_bytes());
            }
            Item::Syscall => code.extend_from_slice(&[0xCD, 0x80]),
            Item::Return => code.push(0xC3),
            Item::Nop => code.push(0x90),
        }
    }

    if code.len() != code_size || entry >= code_size {
        return Err(ToolchainError::InvalidAssembly);
    }

    let mut object = Vec::with_capacity(24 + code.len());
    object.extend_from_slice(OBJECT_MAGIC);
    object.extend_from_slice(&(entry as u64).to_le_bytes());
    object.extend_from_slice(&(code.len() as u64).to_le_bytes());
    object.extend_from_slice(&code);
    Ok(object)
}

pub fn link(object: &[u8]) -> Result<Vec<u8>, ToolchainError> {
    if object.len() < 24 || object.get(..8) != Some(OBJECT_MAGIC) {
        return Err(ToolchainError::InvalidObject);
    }
    let entry_offset = read_u64(object, 8)? as usize;
    let code_size = read_u64(object, 16)? as usize;
    let code_end = 24usize
        .checked_add(code_size)
        .ok_or(ToolchainError::OutOfBounds)?;
    if code_size == 0
        || code_size > MAX_CODE_SIZE
        || code_end != object.len()
        || entry_offset >= code_size
    {
        return Err(ToolchainError::InvalidObject);
    }

    let output_size = ELF_LOAD_OFFSET
        .checked_add(code_size)
        .ok_or(ToolchainError::OutOfBounds)?;
    let mut executable = Vec::with_capacity(output_size);
    executable.resize(ELF_LOAD_OFFSET, 0);

    let ident = &mut executable[..16];
    ident[..4].copy_from_slice(b"\x7FELF");
    ident[4] = 2;
    ident[5] = 1;
    ident[6] = 1;

    put_u16(&mut executable, 16, 2);
    put_u16(&mut executable, 18, 62);
    put_u32(&mut executable, 20, 1);
    put_u64(
        &mut executable,
        24,
        ELF_BASE
            .checked_add(entry_offset as u64)
            .ok_or(ToolchainError::OutOfBounds)?,
    );
    put_u64(&mut executable, 32, ELF_HEADER_SIZE as u64);
    put_u16(&mut executable, 52, ELF_HEADER_SIZE as u16);
    put_u16(&mut executable, 54, PROGRAM_HEADER_SIZE as u16);
    put_u16(&mut executable, 56, 1);

    let ph = ELF_HEADER_SIZE;
    put_u32(&mut executable, ph, 1);
    put_u32(&mut executable, ph + 4, 5);
    put_u64(&mut executable, ph + 8, ELF_LOAD_OFFSET as u64);
    put_u64(&mut executable, ph + 16, ELF_BASE);
    put_u64(&mut executable, ph + 24, ELF_BASE);
    put_u64(&mut executable, ph + 32, code_size as u64);
    put_u64(&mut executable, ph + 40, code_size as u64);
    put_u64(&mut executable, ph + 48, 0x1000);
    executable.extend_from_slice(&object[24..]);
    Ok(executable)
}

pub fn compile(source: &[u8]) -> Result<Vec<u8>, ToolchainError> {
    compile_with_includes(source, "/", |_| Err(FsError::NotFound))
}

pub fn compile_with_includes(
    source: &[u8],
    source_path: &str,
    read_include: impl FnMut(&str) -> Result<Vec<u8>, FsError>,
) -> Result<Vec<u8>, ToolchainError> {
    if source.len() > MAX_SOURCE_SIZE {
        return Err(ToolchainError::SourceTooLarge);
    }
    let source = core::str::from_utf8(source).map_err(|_| ToolchainError::InvalidUtf8)?;
    let preprocessor = CPreprocessor::new(read_include);
    let (source, macros) = preprocessor.process(source_path, source)?;
    let mut parser = CParser::new(&source, &macros)?;
    let program = parser.parse()?;
    let assembly = program.to_assembly();
    link(&assemble(assembly.as_bytes())?)
}

struct CPreprocessor<F> {
    macros: BTreeMap<String, String>,
    once_paths: Vec<String>,
    include_files: usize,
    include_bytes: usize,
    read_include: F,
}

struct Conditional {
    parent_active: bool,
    condition: bool,
    in_else: bool,
}

impl<F> CPreprocessor<F>
where
    F: FnMut(&str) -> Result<Vec<u8>, FsError>,
{
    fn new(read_include: F) -> Self {
        Self {
            macros: BTreeMap::new(),
            once_paths: Vec::new(),
            include_files: 0,
            include_bytes: 0,
            read_include,
        }
    }

    fn process(
        mut self,
        source_path: &str,
        source: &str,
    ) -> Result<(String, BTreeMap<String, String>), ToolchainError> {
        let source_path =
            fs::normalize_path(source_path).map_err(|_| ToolchainError::InvalidCSource)?;
        let mut output = String::new();
        self.process_file(&source_path, source, true, 0, &mut output)?;
        Ok((output, self.macros))
    }

    fn process_file(
        &mut self,
        path: &str,
        source: &str,
        is_main: bool,
        depth: usize,
        output: &mut String,
    ) -> Result<(), ToolchainError> {
        if !is_main && self.once_paths.iter().any(|once_path| once_path == path) {
            return Ok(());
        }

        let mut active = true;
        let mut conditionals: Vec<Conditional> = Vec::new();
        for line in source.lines() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('#') {
                if is_main && active {
                    if output.len().saturating_add(line.len() + 1) > MAX_SOURCE_SIZE {
                        return Err(ToolchainError::SourceTooLarge);
                    }
                    output.push_str(line);
                    output.push('\n');
                }
                continue;
            }

            let directive = trimmed[1..].trim_start();
            let keyword_end = directive
                .find(char::is_whitespace)
                .unwrap_or(directive.len());
            let keyword = &directive[..keyword_end];
            let arguments = directive[keyword_end..].trim();
            match keyword {
                "ifdef" | "ifndef" => {
                    let name = parse_macro_name(arguments)?;
                    let defined = self.macros.contains_key(name);
                    let condition = if keyword == "ifdef" {
                        defined
                    } else {
                        !defined
                    };
                    conditionals.push(Conditional {
                        parent_active: active,
                        condition,
                        in_else: false,
                    });
                    active &= condition;
                }
                "else" => {
                    if !arguments.is_empty() {
                        return Err(ToolchainError::UnsupportedPreprocessor);
                    }
                    let conditional = conditionals
                        .last_mut()
                        .ok_or(ToolchainError::InvalidCSource)?;
                    if conditional.in_else {
                        return Err(ToolchainError::InvalidCSource);
                    }
                    conditional.in_else = true;
                    active = conditional.parent_active && !conditional.condition;
                }
                "endif" => {
                    if !arguments.is_empty() {
                        return Err(ToolchainError::UnsupportedPreprocessor);
                    }
                    active = conditionals
                        .pop()
                        .ok_or(ToolchainError::InvalidCSource)?
                        .parent_active;
                }
                _ if !active => {}
                "include" => self.process_include(path, arguments, depth, output)?,
                "define" => self.define_macro(arguments)?,
                "undef" => {
                    let name = parse_macro_name(arguments)?;
                    self.macros.remove(name);
                }
                "pragma" if arguments == "once" => {
                    if !self.once_paths.iter().any(|once_path| once_path == path) {
                        self.once_paths.push(String::from(path));
                    }
                }
                _ => return Err(ToolchainError::UnsupportedPreprocessor),
            }
        }

        if conditionals.is_empty() {
            Ok(())
        } else {
            Err(ToolchainError::InvalidCSource)
        }
    }

    fn define_macro(&mut self, arguments: &str) -> Result<(), ToolchainError> {
        let bytes = arguments.as_bytes();
        let name_end = bytes
            .iter()
            .position(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
            .unwrap_or(bytes.len());
        if name_end == 0 {
            return Err(ToolchainError::InvalidCSource);
        }
        if bytes.get(name_end) == Some(&b'(') {
            return Err(ToolchainError::UnsupportedPreprocessor);
        }
        let name = &arguments[..name_end];
        if !is_c_identifier(name) {
            return Err(ToolchainError::InvalidCSource);
        }
        self.macros.insert(
            String::from(name),
            String::from(arguments[name_end..].trim()),
        );
        Ok(())
    }

    fn process_include(
        &mut self,
        source_path: &str,
        arguments: &str,
        depth: usize,
        output: &mut String,
    ) -> Result<(), ToolchainError> {
        let (name, angled) = parse_include(arguments)?;
        if angled && matches!(name.as_str(), "stddef.h" | "stdint.h") {
            return Ok(());
        }
        if depth >= MAX_INCLUDE_DEPTH {
            return Err(ToolchainError::IncludeDepthExceeded);
        }

        for candidate in include_candidates(source_path, &name, angled)? {
            if self
                .once_paths
                .iter()
                .any(|once_path| once_path == &candidate)
            {
                return Ok(());
            }
            let bytes = match (self.read_include)(&candidate) {
                Ok(bytes) => bytes,
                Err(FsError::NotFound | FsError::NotDirectory) => continue,
                Err(error) => return Err(ToolchainError::Include(error)),
            };
            if bytes.len() > MAX_SOURCE_SIZE {
                return Err(ToolchainError::SourceTooLarge);
            }
            if self.include_files >= MAX_INCLUDE_FILES {
                return Err(ToolchainError::IncludeLimitExceeded);
            }
            if self
                .include_bytes
                .checked_add(bytes.len())
                .map_or(true, |total| total > MAX_INCLUDE_BYTES)
            {
                return Err(ToolchainError::SourceTooLarge);
            }
            self.include_files += 1;
            self.include_bytes += bytes.len();
            let included = core::str::from_utf8(&bytes).map_err(|_| ToolchainError::InvalidUtf8)?;
            self.process_file(&candidate, included, false, depth + 1, output)?;
            return Ok(());
        }
        Err(ToolchainError::IncludeNotFound)
    }
}

fn parse_macro_name(arguments: &str) -> Result<&str, ToolchainError> {
    let name = arguments.trim();
    if is_c_identifier(name) {
        Ok(name)
    } else {
        Err(ToolchainError::UnsupportedPreprocessor)
    }
}

fn is_c_identifier(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn parse_include(arguments: &str) -> Result<(String, bool), ToolchainError> {
    let arguments = arguments.trim();
    let (delimiter, angled) = match arguments.as_bytes().first() {
        Some(b'"') => (b'"', false),
        Some(b'<') => (b'>', true),
        _ => return Err(ToolchainError::InvalidCSource),
    };
    let closing = arguments[1..]
        .find(delimiter as char)
        .map(|index| index + 1)
        .ok_or(ToolchainError::InvalidCSource)?;
    if !arguments[closing + 1..].trim().is_empty() || closing == 1 {
        return Err(ToolchainError::InvalidCSource);
    }
    Ok((String::from(&arguments[1..closing]), angled))
}

fn include_candidates(
    source_path: &str,
    name: &str,
    angled: bool,
) -> Result<Vec<String>, ToolchainError> {
    if name.starts_with('/') {
        return Ok(alloc::vec![
            fs::normalize_path(name).map_err(|_| ToolchainError::InvalidCSource)?
        ]);
    }
    let source_dir = source_path
        .rfind('/')
        .map(|index| &source_path[..index])
        .unwrap_or("/");
    let source_file = format!("{source_dir}/{name}");
    let source_include = format!("{source_dir}/include/{name}");
    let global_include = format!("/include/{name}");
    let ordered = if angled {
        [source_include, global_include, source_file]
    } else {
        [source_file, source_include, global_include]
    };
    let mut candidates = Vec::new();
    for candidate in ordered {
        let candidate =
            fs::normalize_path(&candidate).map_err(|_| ToolchainError::InvalidCSource)?;
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

fn parse_assembly(source: &str) -> Result<Vec<Item>, ToolchainError> {
    let mut items = Vec::new();
    for raw_line in source.lines() {
        let line = strip_assembly_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(label) = line.strip_suffix(':') {
            let label = label.trim();
            if !valid_label(label) {
                return Err(ToolchainError::InvalidAssembly);
            }
            items.push(Item::Label(String::from(label)));
            continue;
        }
        if line.starts_with('.') {
            let (directive, arguments) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            match directive {
                ".text" | ".rodata" | ".data" | ".section" => {}
                ".global" | ".globl" => {
                    if !valid_label(arguments.trim()) {
                        return Err(ToolchainError::InvalidAssembly);
                    }
                }
                ".ascii" | ".asciz" | ".string" => {
                    let mut bytes = parse_quoted(arguments.trim())?;
                    if directive != ".ascii" {
                        bytes.push(0);
                    }
                    items.push(Item::Bytes(bytes));
                }
                ".byte" => {
                    let mut bytes = Vec::new();
                    for value in arguments.split(',') {
                        let value = parse_integer(value.trim().trim_start_matches('$'))?;
                        bytes.push(
                            u8::try_from(value).map_err(|_| ToolchainError::InvalidAssembly)?,
                        );
                    }
                    items.push(Item::Bytes(bytes));
                }
                _ => return Err(ToolchainError::InvalidAssembly),
            }
            continue;
        }

        let (mnemonic, arguments) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let arguments = arguments.trim();
        let item = match mnemonic {
            "mov" => {
                let (left, right) = arguments
                    .split_once(',')
                    .ok_or(ToolchainError::InvalidAssembly)?;
                let right = parse_register(right.trim())?;
                if let Some(immediate) = left.trim().strip_prefix('$') {
                    Item::MoveImmediate {
                        register: right,
                        value: parse_integer(immediate)?,
                    }
                } else {
                    Item::MoveRegister {
                        source: parse_register(left.trim())?,
                        destination: right,
                    }
                }
            }
            "push" if !arguments.is_empty() => Item::PushRegister(parse_register(arguments)?),
            "pop" if !arguments.is_empty() => Item::PopRegister(parse_register(arguments)?),
            "add" | "sub" | "imul" => {
                let (left, right) = arguments
                    .split_once(',')
                    .ok_or(ToolchainError::InvalidAssembly)?;
                let operation = match mnemonic {
                    "add" => BinaryOperation::Add,
                    "sub" => BinaryOperation::Subtract,
                    _ => BinaryOperation::Multiply,
                };
                Item::BinaryRegister {
                    operation,
                    source: parse_register(left.trim())?,
                    destination: parse_register(right.trim())?,
                }
            }
            "test" => {
                let (left, right) = arguments
                    .split_once(',')
                    .ok_or(ToolchainError::InvalidAssembly)?;
                Item::TestRegister {
                    left: parse_register(left.trim())?,
                    right: parse_register(right.trim())?,
                }
            }
            "lea" => {
                let (left, right) = arguments
                    .split_once(',')
                    .ok_or(ToolchainError::InvalidAssembly)?;
                let label = left
                    .trim()
                    .strip_suffix("(%rip)")
                    .ok_or(ToolchainError::InvalidAssembly)?
                    .trim();
                if !valid_label(label) {
                    return Err(ToolchainError::InvalidAssembly);
                }
                Item::LeaRip {
                    label: String::from(label),
                    destination: parse_register(right.trim())?,
                }
            }
            "call" | "jmp" => {
                if !valid_label(arguments) {
                    return Err(ToolchainError::InvalidAssembly);
                }
                Item::RelativeBranch {
                    label: String::from(arguments),
                    call: mnemonic == "call",
                }
            }
            "je" | "jz" | "jne" | "jnz" => {
                if !valid_label(arguments) {
                    return Err(ToolchainError::InvalidAssembly);
                }
                Item::ConditionalBranch {
                    label: String::from(arguments),
                    condition: if matches!(mnemonic, "je" | "jz") {
                        BranchCondition::Equal
                    } else {
                        BranchCondition::NotEqual
                    },
                }
            }
            "int" if parse_integer(arguments.trim_start_matches('$'))? == 0x80 => Item::Syscall,
            "ret" if arguments.is_empty() => Item::Return,
            "nop" if arguments.is_empty() => Item::Nop,
            _ => return Err(ToolchainError::InvalidAssembly),
        };
        items.push(item);
    }
    Ok(items)
}

fn strip_assembly_comment(line: &str) -> &str {
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' && quoted {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
        } else if character == '#' && !quoted {
            return &line[..index];
        }
    }
    line
}

fn parse_quoted(source: &str) -> Result<Vec<u8>, ToolchainError> {
    let bytes = source.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || *bytes.last().unwrap() != b'"' {
        return Err(ToolchainError::InvalidAssembly);
    }
    unescape(&bytes[1..bytes.len() - 1]).map_err(|_| ToolchainError::InvalidAssembly)
}

fn unescape(source: &[u8]) -> Result<Vec<u8>, ToolchainError> {
    let mut output = Vec::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        if source[index] != b'\\' {
            output.push(source[index]);
            index += 1;
            continue;
        }
        index += 1;
        let escaped = *source.get(index).ok_or(ToolchainError::InvalidAssembly)?;
        output.push(match escaped {
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'0' => 0,
            b'\\' => b'\\',
            b'"' => b'"',
            _ => return Err(ToolchainError::InvalidAssembly),
        });
        index += 1;
    }
    Ok(output)
}

fn valid_label(label: &str) -> bool {
    let mut characters = label.chars();
    matches!(characters.next(), Some(c) if c.is_ascii_alphabetic() || matches!(c, '_' | '.' | '$'))
        && characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$'))
}

fn parse_register(register: &str) -> Result<u8, ToolchainError> {
    match register {
        "%rax" => Ok(0),
        "%rdx" => Ok(2),
        "%rsi" => Ok(6),
        "%rdi" => Ok(7),
        _ => Err(ToolchainError::InvalidAssembly),
    }
}

fn parse_integer(value: &str) -> Result<u64, ToolchainError> {
    if let Some(hex) = value.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).map_err(|_| ToolchainError::InvalidAssembly)
    } else {
        value
            .parse::<u64>()
            .map_err(|_| ToolchainError::InvalidAssembly)
    }
}

fn relative_displacement(target: usize, next_instruction: usize) -> Result<i32, ToolchainError> {
    let displacement = target as i128 - next_instruction as i128;
    i32::try_from(displacement).map_err(|_| ToolchainError::OutOfBounds)
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, ToolchainError> {
    let end = offset.checked_add(8).ok_or(ToolchainError::OutOfBounds)?;
    let raw: [u8; 8] = bytes
        .get(offset..end)
        .ok_or(ToolchainError::InvalidObject)?
        .try_into()
        .map_err(|_| ToolchainError::InvalidObject)?;
    Ok(u64::from_le_bytes(raw))
}

#[derive(Debug, Clone)]
enum CToken {
    Identifier(String),
    Number(u64),
    String(Vec<u8>),
    Symbol(u8),
}

enum CExpression {
    Number(u64),
    Register(CRegister),
    Negate(Box<Self>),
    Binary {
        operator: COperator,
        left: Box<Self>,
        right: Box<Self>,
    },
}

#[derive(Clone, Copy)]
enum COperator {
    Add,
    Subtract,
    Multiply,
}

#[derive(Clone, Copy)]
enum CRegister {
    Rsi,
    Rdx,
}

impl CRegister {
    fn name(self) -> &'static str {
        match self {
            Self::Rsi => "%rsi",
            Self::Rdx => "%rdx",
        }
    }
}

enum CStatement {
    Declare {
        register: CRegister,
        expression: CExpression,
    },
    Assign {
        register: CRegister,
        expression: CExpression,
    },
    Write {
        string_index: usize,
        length: usize,
        saved_registers: Vec<CRegister>,
    },
    If {
        condition: CExpression,
        then_body: Vec<CStatement>,
        else_body: Vec<CStatement>,
    },
    While {
        condition: CExpression,
        body: Vec<CStatement>,
    },
}

impl CExpression {
    fn emit(&self, assembly: &mut String) {
        match self {
            Self::Number(value) => assembly.push_str(&format!("mov ${value}, %rax\n")),
            Self::Register(register) => {
                assembly.push_str(&format!("mov {}, %rax\n", register.name()));
            }
            Self::Negate(expression) => {
                expression.emit(assembly);
                assembly.push_str("push %rax\nmov $0, %rax\npop %rdi\nsub %rdi, %rax\n");
            }
            Self::Binary {
                operator,
                left,
                right,
            } => {
                left.emit(assembly);
                assembly.push_str("push %rax\n");
                right.emit(assembly);
                assembly.push_str("pop %rdi\n");
                match operator {
                    COperator::Add => assembly.push_str("add %rdi, %rax\n"),
                    COperator::Subtract => {
                        assembly.push_str("sub %rax, %rdi\nmov %rdi, %rax\n");
                    }
                    COperator::Multiply => assembly.push_str("imul %rdi, %rax\n"),
                }
            }
        }
    }
}

struct CProgram {
    statements: Vec<CStatement>,
    strings: Vec<Vec<u8>>,
    return_expression: CExpression,
}

impl CProgram {
    fn to_assembly(&self) -> String {
        let mut assembly = String::from(
            ".global _start\n.text\n_start:\ncall main\nmov %rax, %rdi\nmov $0, %rax\nint $0x80\nmain:\n",
        );
        let mut next_label = 0;
        emit_statements(&self.statements, &mut assembly, &mut next_label);
        self.return_expression.emit(&mut assembly);
        assembly.push_str("ret\n.rodata\n");
        for (index, bytes) in self.strings.iter().enumerate() {
            assembly.push_str(&format!(".Lstring{index}:\n.byte "));
            for byte in bytes {
                assembly.push_str(&format!("{byte},"));
            }
            assembly.push_str("0\n");
        }
        assembly
    }
}

fn emit_statements(statements: &[CStatement], assembly: &mut String, next_label: &mut usize) {
    for statement in statements {
        match statement {
            CStatement::Declare {
                register,
                expression,
            }
            | CStatement::Assign {
                register,
                expression,
            } => {
                expression.emit(assembly);
                assembly.push_str(&format!("mov %rax, {}\n", register.name()));
            }
            CStatement::Write {
                string_index,
                length,
                saved_registers,
            } => {
                for register in saved_registers {
                    assembly.push_str(&format!("push {}\n", register.name()));
                }
                assembly.push_str("mov $1, %rax\nmov $1, %rdi\n");
                assembly.push_str(&format!("lea .Lstring{string_index}(%rip), %rsi\n"));
                assembly.push_str(&format!("mov ${length}, %rdx\nint $0x80\n"));
                for register in saved_registers.iter().rev() {
                    assembly.push_str(&format!("pop {}\n", register.name()));
                }
            }
            CStatement::If {
                condition,
                then_body,
                else_body,
            } => {
                let label = *next_label;
                *next_label += 1;
                condition.emit(assembly);
                assembly.push_str(&format!("test %rax, %rax\nje .Lif_else{label}\n"));
                emit_statements(then_body, assembly, next_label);
                if !else_body.is_empty() {
                    assembly.push_str(&format!("jmp .Lif_end{label}\n"));
                }
                assembly.push_str(&format!(".Lif_else{label}:\n"));
                emit_statements(else_body, assembly, next_label);
                if !else_body.is_empty() {
                    assembly.push_str(&format!(".Lif_end{label}:\n"));
                }
            }
            CStatement::While { condition, body } => {
                let label = *next_label;
                *next_label += 1;
                assembly.push_str(&format!(".Lwhile_start{label}:\n"));
                condition.emit(assembly);
                assembly.push_str(&format!("test %rax, %rax\nje .Lwhile_end{label}\n"));
                emit_statements(body, assembly, next_label);
                assembly.push_str(&format!("jmp .Lwhile_start{label}\n.Lwhile_end{label}:\n"));
            }
        }
    }
}

struct CParser<'a> {
    tokens: Vec<CToken>,
    index: usize,
    macros: &'a BTreeMap<String, String>,
    variables: BTreeMap<String, CRegister>,
    local_registers: Vec<CRegister>,
}

impl<'a> CParser<'a> {
    fn new(source: &str, macros: &'a BTreeMap<String, String>) -> Result<Self, ToolchainError> {
        Ok(Self {
            tokens: tokenize_c(source)?,
            index: 0,
            macros,
            variables: BTreeMap::new(),
            local_registers: Vec::new(),
        })
    }

    fn parse(&mut self) -> Result<CProgram, ToolchainError> {
        self.expect_identifier("int")?;
        self.expect_identifier("main")?;
        self.expect_symbol(b'(')?;
        if self.peek_identifier("void") {
            self.index += 1;
        }
        self.expect_symbol(b')')?;
        self.expect_symbol(b'{')?;

        let mut statements = Vec::new();
        let mut strings = Vec::new();
        let mut return_expression = None;
        while !self.peek_symbol(b'}') {
            if self.peek_identifier("return") {
                self.index += 1;
                return_expression = Some(self.parse_expression(0)?);
                self.expect_symbol(b';')?;
                break;
            }
            statements.push(self.parse_statement(0, &mut strings)?);
        }
        self.expect_symbol(b'}')?;
        if self.index != self.tokens.len() {
            return Err(ToolchainError::UnsupportedC);
        }
        Ok(CProgram {
            statements,
            strings,
            return_expression: return_expression.ok_or(ToolchainError::InvalidCSource)?,
        })
    }

    fn parse_statement(
        &mut self,
        control_depth: usize,
        strings: &mut Vec<Vec<u8>>,
    ) -> Result<CStatement, ToolchainError> {
        if self.peek_identifier("if") {
            if control_depth >= MAX_C_CONTROL_DEPTH {
                return Err(ToolchainError::UnsupportedC);
            }
            self.index += 1;
            self.expect_symbol(b'(')?;
            let condition = self.parse_expression(0)?;
            self.expect_symbol(b')')?;
            let then_body = self.parse_block(control_depth + 1, strings)?;
            let else_body = if self.peek_identifier("else") {
                self.index += 1;
                self.parse_block(control_depth + 1, strings)?
            } else {
                Vec::new()
            };
            return Ok(CStatement::If {
                condition,
                then_body,
                else_body,
            });
        }
        if self.peek_identifier("while") {
            if control_depth >= MAX_C_CONTROL_DEPTH {
                return Err(ToolchainError::UnsupportedC);
            }
            self.index += 1;
            self.expect_symbol(b'(')?;
            let condition = self.parse_expression(0)?;
            self.expect_symbol(b')')?;
            let body = self.parse_block(control_depth + 1, strings)?;
            return Ok(CStatement::While { condition, body });
        }
        if self.peek_identifier("int") {
            if control_depth != 0 {
                return Err(ToolchainError::UnsupportedC);
            }
            self.index += 1;
            let Some(CToken::Identifier(name)) = self.next_token() else {
                return Err(ToolchainError::InvalidCSource);
            };
            if self.variables.contains_key(&name) {
                return Err(ToolchainError::InvalidCSource);
            }
            self.expect_symbol(b'=')?;
            let expression = self.parse_expression(0)?;
            self.expect_symbol(b';')?;
            let register = match self.variables.len() {
                0 => CRegister::Rsi,
                1 => CRegister::Rdx,
                _ => return Err(ToolchainError::UnsupportedC),
            };
            self.variables.insert(name, register);
            self.local_registers.push(register);
            return Ok(CStatement::Declare {
                register,
                expression,
            });
        }
        if let Some(register) = self.peek_assignment() {
            self.index += 2;
            let expression = self.parse_expression(0)?;
            self.expect_symbol(b';')?;
            return Ok(CStatement::Assign {
                register,
                expression,
            });
        }

        self.expect_identifier("oct_write")?;
        self.expect_symbol(b'(')?;
        if self.expect_number()? != 1 {
            return Err(ToolchainError::UnsupportedC);
        }
        self.expect_symbol(b',')?;
        let message = self.expect_string()?;
        self.expect_symbol(b',')?;
        let length =
            usize::try_from(self.expect_number()?).map_err(|_| ToolchainError::OutOfBounds)?;
        if length > message.len() {
            return Err(ToolchainError::InvalidCSource);
        }
        self.expect_symbol(b')')?;
        self.expect_symbol(b';')?;
        let string_index = strings.len();
        strings.push(message);
        Ok(CStatement::Write {
            string_index,
            length,
            saved_registers: self.local_registers.clone(),
        })
    }

    fn parse_block(
        &mut self,
        control_depth: usize,
        strings: &mut Vec<Vec<u8>>,
    ) -> Result<Vec<CStatement>, ToolchainError> {
        self.expect_symbol(b'{')?;
        let mut statements = Vec::new();
        while !self.peek_symbol(b'}') {
            if self.peek_identifier("return") {
                return Err(ToolchainError::UnsupportedC);
            }
            statements.push(self.parse_statement(control_depth, strings)?);
        }
        self.expect_symbol(b'}')?;
        Ok(statements)
    }

    fn parse_expression(&mut self, minimum_precedence: u8) -> Result<CExpression, ToolchainError> {
        let mut operations = 0;
        self.parse_expression_nested(minimum_precedence, 0, &mut operations)
    }

    fn parse_expression_nested(
        &mut self,
        minimum_precedence: u8,
        depth: usize,
        operations: &mut usize,
    ) -> Result<CExpression, ToolchainError> {
        if depth > MAX_C_EXPRESSION_DEPTH {
            return Err(ToolchainError::UnsupportedC);
        }
        let mut left = match self.next_token() {
            Some(CToken::Number(number)) => CExpression::Number(number),
            Some(CToken::Identifier(name)) => match self.variables.get(&name) {
                Some(register) => CExpression::Register(*register),
                None => CExpression::Number(
                    self.macros
                        .get(&name)
                        .and_then(|value| parse_macro_integer(value))
                        .ok_or(ToolchainError::UnsupportedC)?,
                ),
            },
            Some(CToken::Symbol(b'-')) => {
                *operations += 1;
                if *operations > MAX_C_EXPRESSION_OPERATIONS {
                    return Err(ToolchainError::UnsupportedC);
                }
                CExpression::Negate(Box::new(self.parse_expression_nested(
                    3,
                    depth + 1,
                    operations,
                )?))
            }
            Some(CToken::Symbol(b'+')) => {
                *operations += 1;
                if *operations > MAX_C_EXPRESSION_OPERATIONS {
                    return Err(ToolchainError::UnsupportedC);
                }
                self.parse_expression_nested(3, depth + 1, operations)?
            }
            Some(CToken::Symbol(b'(')) => {
                let expression = self.parse_expression_nested(0, depth + 1, operations)?;
                self.expect_symbol(b')')?;
                expression
            }
            _ => return Err(ToolchainError::InvalidCSource),
        };

        loop {
            let Some(CToken::Symbol(symbol)) = self.tokens.get(self.index) else {
                break;
            };
            let (precedence, operator) = match *symbol {
                b'+' => (1, COperator::Add),
                b'-' => (1, COperator::Subtract),
                b'*' => (2, COperator::Multiply),
                _ => break,
            };
            if precedence < minimum_precedence {
                break;
            }
            self.index += 1;
            *operations += 1;
            if *operations > MAX_C_EXPRESSION_OPERATIONS {
                return Err(ToolchainError::UnsupportedC);
            }
            let right = self.parse_expression_nested(precedence + 1, depth + 1, operations)?;
            left = CExpression::Binary {
                operator,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn next_token(&mut self) -> Option<CToken> {
        let token = self.tokens.get(self.index).cloned();
        self.index += usize::from(token.is_some());
        token
    }

    fn expect_identifier(&mut self, expected: &str) -> Result<(), ToolchainError> {
        match self.next_token() {
            Some(CToken::Identifier(identifier)) if identifier == expected => Ok(()),
            _ => Err(ToolchainError::InvalidCSource),
        }
    }

    fn expect_symbol(&mut self, expected: u8) -> Result<(), ToolchainError> {
        match self.next_token() {
            Some(CToken::Symbol(symbol)) if symbol == expected => Ok(()),
            _ => Err(ToolchainError::InvalidCSource),
        }
    }

    fn expect_number(&mut self) -> Result<u64, ToolchainError> {
        match self.next_token() {
            Some(CToken::Number(number)) => Ok(number),
            Some(CToken::Identifier(name)) if name == "OCT_STDOUT" => Ok(1),
            Some(CToken::Identifier(name)) => self
                .macros
                .get(&name)
                .and_then(|value| parse_macro_integer(value))
                .ok_or(ToolchainError::UnsupportedC),
            _ => Err(ToolchainError::InvalidCSource),
        }
    }

    fn expect_string(&mut self) -> Result<Vec<u8>, ToolchainError> {
        match self.next_token() {
            Some(CToken::String(message)) => Ok(message),
            Some(CToken::Identifier(name)) => self
                .macros
                .get(&name)
                .ok_or(ToolchainError::UnsupportedC)
                .and_then(|value| parse_macro_string(value)),
            _ => Err(ToolchainError::InvalidCSource),
        }
    }

    fn peek_identifier(&self, expected: &str) -> bool {
        matches!(self.tokens.get(self.index), Some(CToken::Identifier(identifier)) if identifier == expected)
    }

    fn peek_symbol(&self, expected: u8) -> bool {
        matches!(self.tokens.get(self.index), Some(CToken::Symbol(symbol)) if *symbol == expected)
    }

    fn peek_assignment(&self) -> Option<CRegister> {
        let Some(CToken::Identifier(name)) = self.tokens.get(self.index) else {
            return None;
        };
        if !matches!(self.tokens.get(self.index + 1), Some(CToken::Symbol(b'='))) {
            return None;
        }
        self.variables.get(name).copied()
    }
}

fn parse_macro_integer(value: &str) -> Option<u64> {
    let value = value
        .trim()
        .trim_end_matches(|character| matches!(character, 'u' | 'U' | 'l' | 'L'));
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).ok()
    } else {
        value.parse().ok()
    }
}

fn parse_macro_string(value: &str) -> Result<Vec<u8>, ToolchainError> {
    let bytes = value.trim().as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || *bytes.last().unwrap() != b'"' {
        return Err(ToolchainError::UnsupportedC);
    }
    unescape(&bytes[1..bytes.len() - 1]).map_err(|_| ToolchainError::UnsupportedC)
}

fn tokenize_c(source: &str) -> Result<Vec<CToken>, ToolchainError> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            byte if byte.is_ascii_whitespace() => index += 1,
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                if index + 1 >= bytes.len() {
                    return Err(ToolchainError::InvalidCSource);
                }
                index += 2;
            }
            b'"' => {
                let start = index + 1;
                index = start;
                let mut escaped = false;
                while index < bytes.len() {
                    if !escaped && bytes[index] == b'"' {
                        break;
                    }
                    if !escaped && bytes[index] == b'\\' {
                        escaped = true;
                    } else {
                        escaped = false;
                    }
                    index += 1;
                }
                if index >= bytes.len() {
                    return Err(ToolchainError::InvalidCSource);
                }
                tokens.push(CToken::String(
                    unescape(&bytes[start..index]).map_err(|_| ToolchainError::InvalidCSource)?,
                ));
                index += 1;
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                let identifier = core::str::from_utf8(&bytes[start..index])
                    .map_err(|_| ToolchainError::InvalidUtf8)?;
                tokens.push(CToken::Identifier(String::from(identifier)));
            }
            byte if byte.is_ascii_digit() => {
                let radix = if byte == b'0'
                    && bytes
                        .get(index + 1)
                        .is_some_and(|next| matches!(next, b'x' | b'X'))
                {
                    index += 2;
                    16
                } else {
                    10
                };
                let start = index;
                while index < bytes.len()
                    && if radix == 16 {
                        bytes[index].is_ascii_hexdigit()
                    } else {
                        bytes[index].is_ascii_digit()
                    }
                {
                    index += 1;
                }
                if index == start {
                    return Err(ToolchainError::InvalidCSource);
                }
                let number = core::str::from_utf8(&bytes[start..index])
                    .map_err(|_| ToolchainError::InvalidUtf8)?;
                let value = u64::from_str_radix(number, radix)
                    .map_err(|_| ToolchainError::InvalidCSource)?;
                while index < bytes.len() && matches!(bytes[index], b'u' | b'U' | b'l' | b'L') {
                    index += 1;
                }
                tokens.push(CToken::Number(value));
            }
            symbol @ (b'(' | b')' | b'{' | b'}' | b',' | b';' | b'+' | b'-' | b'*' | b'=') => {
                tokens.push(CToken::Symbol(symbol));
                index += 1;
            }
            _ => return Err(ToolchainError::UnsupportedC),
        }
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::{ToolchainError, assemble, compile, compile_with_includes, link};
    use crate::fs::FsError;

    #[test]
    fn assembler_encodes_stack_and_integer_operations() {
        let object = assemble(
            b".global _start\n_start:\npush %rax\nmov $3, %rax\npop %rdi\nadd %rdi, %rax\nsub %rdi, %rax\nimul %rdi, %rax\nret\n",
        )
        .unwrap();
        assert_eq!(
            &object[24..],
            &[
                0x50, 0x48, 0xB8, 3, 0, 0, 0, 0, 0, 0, 0, 0x5F, 0x48, 0x01, 0xF8, 0x48, 0x29, 0xF8,
                0x48, 0x0F, 0xAF, 0xC7, 0xC3,
            ]
        );
    }

    #[test]
    fn assembler_encodes_test_and_conditional_branch_displacements() {
        let object = assemble(
            b".global _start\n_start:\ntest %rax, %rax\nje done\nmov $1, %rax\njmp done\ndone:\nret\n",
        )
        .unwrap();
        assert_eq!(
            &object[24..],
            &[
                0x48, 0x85, 0xC0, 0x0F, 0x84, 0x0F, 0, 0, 0, 0x48, 0xB8, 1, 0, 0, 0, 0, 0, 0, 0,
                0xE9, 0, 0, 0, 0, 0xC3,
            ]
        );
    }

    #[test]
    fn assembles_and_links_a_static_elf() {
        let object =
            assemble(b".global _start\n.text\n_start:\nmov $0, %rax\nmov $9, %rdi\nint $0x80\n")
                .unwrap();
        let elf = link(&object).unwrap();
        assert_eq!(&elf[..4], b"\x7FELF");
        assert_eq!(u16::from_le_bytes([elf[16], elf[17]]), 2);
        assert_eq!(u16::from_le_bytes([elf[18], elf[19]]), 62);
        assert_eq!(
            u64::from_le_bytes(elf[24..32].try_into().unwrap()),
            0x400000
        );
    }

    #[test]
    fn compiles_c_subset_with_console_output_and_exit() {
        let elf =
            compile(b"int main(void) { oct_write(OCT_STDOUT, \"ok\\n\", 3); return 0; }").unwrap();
        assert_eq!(&elf[..4], b"\x7FELF");
        assert_eq!(u16::from_le_bytes([elf[16], elf[17]]), 2);
    }

    #[test]
    fn compiles_integer_return_expressions_with_precedence_and_hex_literals() {
        let elf = compile(b"int main(void) { return 2 + 3 * (4 - 1) + 0x2aU; }").unwrap();
        let text = &elf[super::ELF_LOAD_OFFSET..];
        assert!(
            text.windows(4)
                .any(|bytes| bytes == [0x48, 0x0F, 0xAF, 0xC7])
        );
        assert!(text.windows(3).any(|bytes| bytes == [0x48, 0x29, 0xC7]));
        assert!(text.windows(3).any(|bytes| bytes == [0x48, 0x01, 0xF8]));
        assert_eq!(
            compile(b"int main(void) { return unknown + 1; }"),
            Err(ToolchainError::UnsupportedC)
        );
    }

    #[test]
    fn compiles_local_variables_and_preserves_them_across_console_writes() {
        let elf = compile(
            b"int main(void) { int value = 5; int doubled = value * 2; oct_write(OCT_STDOUT, \"ok\", 2); value = doubled + 1; return value - 3; }",
        )
        .unwrap();
        let text = &elf[super::ELF_LOAD_OFFSET..];
        assert!(text.windows(2).any(|bytes| bytes == [0x56, 0x52]));
        assert!(text.windows(2).any(|bytes| bytes == [0x5A, 0x5E]));
        assert!(text.windows(3).any(|bytes| bytes == [0x48, 0x89, 0xF0]));
        assert!(text.windows(3).any(|bytes| bytes == [0x48, 0x89, 0xD0]));
        assert_eq!(
            compile(
                b"int main(void) { int first = 1; int second = 2; int third = 3; return first; }"
            ),
            Err(ToolchainError::UnsupportedC)
        );
    }

    #[test]
    fn compiles_if_else_and_while_with_nested_labels() {
        let elf = compile(
            b"int main(void) { int count = 3; while (count) { oct_write(OCT_STDOUT, \"tick\\n\", 5); count = count - 1; } if (count) { oct_write(OCT_STDOUT, \"unexpected\\n\", 11); } else { oct_write(OCT_STDOUT, \"done\\n\", 5); } return count; }",
        )
        .unwrap();
        let text = &elf[super::ELF_LOAD_OFFSET..];
        assert!(text.windows(3).any(|bytes| bytes == [0x48, 0x85, 0xC0]));
        assert!(text.windows(2).any(|bytes| bytes == [0x0F, 0x84]));
        assert!(text.contains(&0xE9));
        assert_eq!(
            compile(b"int main(void) { if (1) { return 0; } return 1; }"),
            Err(ToolchainError::UnsupportedC)
        );
        assert_eq!(
            compile(b"int main(void) { if (1) { int nested = 1; } return 0; }"),
            Err(ToolchainError::UnsupportedC)
        );
    }

    #[test]
    fn rejects_control_flow_nesting_over_limit() {
        let mut source = alloc::string::String::from("int main(void) {");
        for _ in 0..=super::MAX_C_CONTROL_DEPTH {
            source.push_str("if (1) {");
        }
        source.push_str("oct_write(OCT_STDOUT, \"x\", 1);");
        for _ in 0..=super::MAX_C_CONTROL_DEPTH {
            source.push('}');
        }
        source.push_str("return 0;}");
        assert_eq!(
            compile(source.as_bytes()),
            Err(ToolchainError::UnsupportedC)
        );
    }

    #[test]
    fn rejects_expressions_exceeding_parser_limits() {
        let mut source = alloc::string::String::from("int main(void) { return ");
        for _ in 0..=super::MAX_C_EXPRESSION_DEPTH {
            source.push('(');
        }
        source.push('1');
        for _ in 0..=super::MAX_C_EXPRESSION_DEPTH {
            source.push(')');
        }
        source.push_str("; }");
        assert_eq!(
            compile(source.as_bytes()),
            Err(ToolchainError::UnsupportedC)
        );
    }

    #[test]
    fn compiles_with_vfs_header_guards_and_literal_macros() {
        let header = b"#ifndef OCTAGRAM_H\n#define OCTAGRAM_H\n#include <stddef.h>\n#define OCT_STDOUT 1\n#define MESSAGE \"ok\\n\"\n#define MESSAGE_LENGTH 3\n#define EXIT_STATUS 7\nstruct ignored_header_type { int value; };\n#endif\n";
        let source = b"#include \"octagram.h\"\n#include \"octagram.h\"\nint main(void) { oct_write(OCT_STDOUT, MESSAGE, MESSAGE_LENGTH); return EXIT_STATUS; }\n";
        let elf = compile_with_includes(source, "/src/main.c", |path| {
            if path == "/src/include/octagram.h" {
                Ok(header.to_vec())
            } else {
                Err(FsError::NotFound)
            }
        })
        .unwrap();
        assert_eq!(&elf[..4], b"\x7FELF");

        let project_header = include_bytes!("../../user/include/octagram.h");
        let elf = compile_with_includes(
            b"#include <octagram.h>\nint main(void) { oct_write(OCT_STDOUT, \"ok\\n\", 3); return 0; }\n",
            "/src/main.c",
            |path| {
                if path == "/include/octagram.h" {
                    Ok(project_header.to_vec())
                } else {
                    Err(FsError::NotFound)
                }
            },
        )
        .unwrap();
        assert_eq!(&elf[..4], b"\x7FELF");
    }

    #[test]
    fn reports_missing_c_header() {
        assert_eq!(
            compile_with_includes(
                b"#include \"missing.h\"\nint main(void) { return 0; }",
                "/src/main.c",
                |_| Err(FsError::NotFound),
            ),
            Err(ToolchainError::IncludeNotFound)
        );
    }

    #[test]
    fn rejects_c_features_outside_the_documented_subset() {
        assert_eq!(
            compile(b"int main(void) { return 1 / 2; }"),
            Err(ToolchainError::UnsupportedC)
        );
    }

    #[test]
    fn rejects_objects_with_trailing_data() {
        let mut object = assemble(b".global _start\n_start:\nmov $0, %rax\nint $0x80\n").unwrap();
        object.push(0);
        assert_eq!(link(&object), Err(ToolchainError::InvalidObject));
    }
}
