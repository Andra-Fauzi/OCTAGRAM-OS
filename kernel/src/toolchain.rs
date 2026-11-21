//! Small native assembler, linker, and C-subset compiler for OCTAGRAM.

use crate::fs::{self, FsError};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

const OBJECT_MAGIC: &[u8; 8] = b"OCTOBJ1\0";
const MAX_SOURCE_SIZE: usize = 8 * 1024;
const MAX_ASSEMBLY_SIZE: usize = 256 * 1024;
const MAX_C_EXPRESSION_DEPTH: usize = 64;
const MAX_C_EXPRESSION_OPERATIONS: usize = 64;
const MAX_C_CONTROL_DEPTH: usize = 32;
const MAX_INCLUDE_DEPTH: usize = 8;
const MAX_INCLUDE_FILES: usize = 32;
const MAX_INCLUDE_BYTES: usize = 32 * 1024;
const MAX_CODE_SIZE: usize = 512 * 1024;
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
    InvalidElfObject,
    UnsupportedRelocation,
    RelocationOverflow,
    UndefinedSymbol,
    DuplicateSymbol,
    UnsupportedCommonSymbol,
    NoEntryPoint,
}

#[derive(Debug)]
enum Item {
    Label(String),
    Bytes(Vec<u8>),
    Align(usize),
    MoveImmediate {
        register: u8,
        value: u64,
    },
    MoveImmediate32 {
        register: u8,
        value: u32,
    },
    MoveImmediateByte {
        register: u8,
        value: u8,
    },
    MoveRegister {
        source: u8,
        destination: u8,
    },
    MoveRegister32 {
        source: u8,
        destination: u8,
    },
    MoveZeroExtendByteToRegister {
        source: u8,
        destination: u8,
    },
    MoveRegisterToMemory {
        source: u8,
        memory: MemoryOperand,
    },
    MoveRegister32ToMemory {
        source: u8,
        memory: MemoryOperand,
    },
    MoveMemoryToRegister {
        memory: MemoryOperand,
        destination: u8,
    },
    MoveMemory32ToRegister {
        memory: MemoryOperand,
        destination: u8,
    },
    SignExtend32MemoryToRegister {
        memory: MemoryOperand,
        destination: u8,
    },
    LeaMemory {
        memory: MemoryOperand,
        destination: u8,
    },
    ImmediateBinary {
        operation: BinaryOperation,
        value: i32,
        destination: u8,
    },
    PushRegister(u8),
    PopRegister(u8),
    BinaryRegister {
        operation: BinaryOperation,
        source: u8,
        destination: u8,
    },
    BinaryRegister32 {
        operation: BinaryOperation,
        source: u8,
        destination: u8,
    },
    TestRegister {
        left: u8,
        right: u8,
    },
    CompareRegister {
        left: u8,
        right: u8,
    },
    CompareRegister32 {
        left: u8,
        right: u8,
    },
    CompareImmediate32 {
        value: i32,
        destination: u8,
    },
    SetConditionByte {
        condition: BranchCondition,
        destination: u8,
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
    RepStosByte,
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
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Debug, Clone, Copy)]
struct MemoryOperand {
    displacement: i32,
    base: u8,
}

impl Item {
    fn encoded_size(&self, offset: usize) -> usize {
        match self {
            Self::Label(_) => 0,
            Self::Bytes(bytes) => bytes.len(),
            Self::Align(alignment) => (alignment - offset % alignment) % alignment,
            Self::MoveImmediate { .. } => 10,
            Self::MoveImmediate32 { register, .. } => 5 + usize::from(*register >= 8),
            Self::MoveImmediateByte { register, .. } => 2 + usize::from(*register >= 4),
            Self::MoveRegister { .. } => 3,
            Self::MoveRegister32 {
                source,
                destination,
            } => 2 + usize::from(*source >= 8 || *destination >= 8),
            Self::MoveZeroExtendByteToRegister { .. } => 4,
            Self::MoveRegisterToMemory { memory, .. }
            | Self::MoveMemoryToRegister { memory, .. }
            | Self::LeaMemory { memory, .. } => 2 + memory_modrm_size(*memory),
            Self::MoveRegister32ToMemory { source, memory }
            | Self::MoveMemory32ToRegister {
                destination: source,
                memory,
            } => usize::from(*source >= 8 || memory.base >= 8) + 1 + memory_modrm_size(*memory),
            Self::SignExtend32MemoryToRegister { memory, .. } => 2 + memory_modrm_size(*memory),
            Self::ImmediateBinary { value, .. } => {
                if i8::try_from(*value).is_ok() {
                    4
                } else {
                    7
                }
            }
            Self::PushRegister(register) | Self::PopRegister(register) => {
                1 + usize::from(*register >= 8)
            }
            Self::BinaryRegister {
                operation: BinaryOperation::Multiply,
                ..
            } => 4,
            Self::BinaryRegister { .. } => 3,
            Self::BinaryRegister32 {
                source,
                destination,
                ..
            } => 2 + usize::from(*source >= 8 || *destination >= 8),
            Self::TestRegister { .. } | Self::CompareRegister { .. } => 3,
            Self::CompareRegister32 { left, right } => 2 + usize::from(*left >= 8 || *right >= 8),
            Self::CompareImmediate32 { value, destination } => {
                usize::from(*destination >= 8)
                    + 2
                    + if i8::try_from(*value).is_ok() { 1 } else { 4 }
            }
            Self::SetConditionByte { destination, .. } => 3 + usize::from(*destination >= 4),
            Self::LeaRip { .. } => 7,
            Self::RelativeBranch { .. } => 5,
            Self::ConditionalBranch { .. } => 6,
            Self::Syscall => 2,
            Self::RepStosByte => 2,
            Self::Return | Self::Nop => 1,
        }
    }
}

pub fn assemble(source: &[u8]) -> Result<Vec<u8>, ToolchainError> {
    if source.len() > MAX_ASSEMBLY_SIZE {
        return Err(ToolchainError::SourceTooLarge);
    }
    let source = core::str::from_utf8(source).map_err(|_| ToolchainError::InvalidUtf8)?;
    let mut items = parse_assembly(source)?;
    if !items
        .iter()
        .any(|item| matches!(item, Item::Label(label) if label == "_start"))
    {
        if !items
            .iter()
            .any(|item| matches!(item, Item::Label(label) if label == "main"))
        {
            return Err(ToolchainError::InvalidAssembly);
        }
        let mut startup = alloc::vec![
            Item::Label(String::from("_start")),
            Item::MoveMemoryToRegister {
                memory: MemoryOperand {
                    displacement: 0,
                    base: 4,
                },
                destination: 7,
            },
            Item::LeaMemory {
                memory: MemoryOperand {
                    displacement: 8,
                    base: 4,
                },
                destination: 6,
            },
            Item::RelativeBranch {
                label: String::from("main"),
                call: true,
            },
            Item::MoveRegister {
                source: 0,
                destination: 7,
            },
            Item::MoveImmediate {
                register: 0,
                value: 0,
            },
            Item::Syscall,
        ];
        startup.append(&mut items);
        items = startup;
    }
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
                    .checked_add(item.encoded_size(code_size))
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
            Item::Align(_) => code.resize(code.len() + item.encoded_size(offset), 0),
            Item::MoveImmediate { register, value } => {
                code.extend_from_slice(&[0x48 | u8::from(*register >= 8), 0xB8 + (*register & 7)]);
                code.extend_from_slice(&value.to_le_bytes());
            }
            Item::MoveImmediate32 { register, value } => {
                if *register >= 8 {
                    code.push(rex(false, 0, 0, *register));
                }
                code.push(0xB8 + (*register & 7));
                code.extend_from_slice(&value.to_le_bytes());
            }
            Item::MoveImmediateByte { register, value } => {
                if *register >= 4 {
                    code.push(rex(false, 0, 0, *register));
                }
                code.extend_from_slice(&[0xB0 + (*register & 7), *value]);
            }
            Item::MoveRegister {
                source,
                destination,
            } => {
                code.push(rex(true, *source, 0, *destination));
                code.extend_from_slice(&[0x89, 0xC0 | ((*source & 7) << 3) | (*destination & 7)]);
            }
            Item::MoveRegister32 {
                source,
                destination,
            } => {
                if *source >= 8 || *destination >= 8 {
                    code.push(rex(false, *source, 0, *destination));
                }
                code.extend_from_slice(&[0x89, 0xC0 | ((*source & 7) << 3) | (*destination & 7)]);
            }
            Item::MoveZeroExtendByteToRegister {
                source,
                destination,
            } => {
                code.push(rex(true, *destination, 0, *source));
                code.extend_from_slice(&[
                    0x0F,
                    0xB6,
                    0xC0 | ((*destination & 7) << 3) | (*source & 7),
                ]);
            }
            Item::MoveRegisterToMemory { source, memory } => {
                code.push(rex(true, *source, 0, memory.base));
                code.push(0x89);
                emit_memory_modrm(&mut code, *source, *memory);
            }
            Item::MoveRegister32ToMemory { source, memory } => {
                if *source >= 8 || memory.base >= 8 {
                    code.push(rex(false, *source, 0, memory.base));
                }
                code.push(0x89);
                emit_memory_modrm(&mut code, *source, *memory);
            }
            Item::MoveMemoryToRegister {
                memory,
                destination,
            } => {
                code.push(rex(true, *destination, 0, memory.base));
                code.push(0x8B);
                emit_memory_modrm(&mut code, *destination, *memory);
            }
            Item::MoveMemory32ToRegister {
                memory,
                destination,
            } => {
                if *destination >= 8 || memory.base >= 8 {
                    code.push(rex(false, *destination, 0, memory.base));
                }
                code.push(0x8B);
                emit_memory_modrm(&mut code, *destination, *memory);
            }
            Item::SignExtend32MemoryToRegister {
                memory,
                destination,
            } => {
                code.push(rex(true, *destination, 0, memory.base));
                code.push(0x63);
                emit_memory_modrm(&mut code, *destination, *memory);
            }
            Item::LeaMemory {
                memory,
                destination,
            } => {
                code.push(rex(true, *destination, 0, memory.base));
                code.push(0x8D);
                emit_memory_modrm(&mut code, *destination, *memory);
            }
            Item::ImmediateBinary {
                operation,
                value,
                destination,
            } => {
                let extension = match operation {
                    BinaryOperation::Add => 0,
                    BinaryOperation::Subtract => 5,
                    BinaryOperation::Multiply => return Err(ToolchainError::InvalidAssembly),
                };
                code.push(rex(true, 0, 0, *destination));
                if let Ok(value) = i8::try_from(*value) {
                    code.extend_from_slice(&[
                        0x83,
                        0xC0 | (extension << 3) | (*destination & 7),
                        value as u8,
                    ]);
                } else {
                    code.extend_from_slice(&[0x81, 0xC0 | (extension << 3) | (*destination & 7)]);
                    code.extend_from_slice(&value.to_le_bytes());
                }
            }
            Item::PushRegister(register) => {
                if *register >= 8 {
                    code.push(0x41);
                }
                code.push(0x50 + (*register & 7));
            }
            Item::PopRegister(register) => {
                if *register >= 8 {
                    code.push(0x41);
                }
                code.push(0x58 + (*register & 7));
            }
            Item::BinaryRegister {
                operation,
                source,
                destination,
            } => match operation {
                BinaryOperation::Add => {
                    code.push(rex(true, *source, 0, *destination));
                    code.extend_from_slice(&[
                        0x01,
                        0xC0 | ((*source & 7) << 3) | (*destination & 7),
                    ]);
                }
                BinaryOperation::Subtract => {
                    code.push(rex(true, *source, 0, *destination));
                    code.extend_from_slice(&[
                        0x29,
                        0xC0 | ((*source & 7) << 3) | (*destination & 7),
                    ]);
                }
                BinaryOperation::Multiply => {
                    code.push(rex(true, *destination, 0, *source));
                    code.extend_from_slice(&[
                        0x0F,
                        0xAF,
                        0xC0 | ((*destination & 7) << 3) | (*source & 7),
                    ]);
                }
            },
            Item::BinaryRegister32 {
                operation,
                source,
                destination,
            } => {
                if *source >= 8 || *destination >= 8 {
                    code.push(rex(false, *source, 0, *destination));
                }
                match operation {
                    BinaryOperation::Add | BinaryOperation::Subtract => {
                        code.extend_from_slice(&[
                            if matches!(operation, BinaryOperation::Add) {
                                0x01
                            } else {
                                0x29
                            },
                            0xC0 | ((*source & 7) << 3) | (*destination & 7),
                        ]);
                    }
                    BinaryOperation::Multiply => {
                        code.extend_from_slice(&[
                            0x0F,
                            0xAF,
                            0xC0 | ((*destination & 7) << 3) | (*source & 7),
                        ]);
                    }
                }
            }
            Item::TestRegister { left, right } => {
                code.push(rex(true, *left, 0, *right));
                code.extend_from_slice(&[0x85, 0xC0 | ((*left & 7) << 3) | (*right & 7)]);
            }
            Item::CompareRegister { left, right } => {
                code.push(rex(true, *left, 0, *right));
                code.extend_from_slice(&[0x39, 0xC0 | ((*left & 7) << 3) | (*right & 7)]);
            }
            Item::CompareRegister32 { left, right } => {
                if *left >= 8 || *right >= 8 {
                    code.push(rex(false, *left, 0, *right));
                }
                code.extend_from_slice(&[0x39, 0xC0 | ((*left & 7) << 3) | (*right & 7)]);
            }
            Item::CompareImmediate32 { value, destination } => {
                if *destination >= 8 {
                    code.push(rex(false, 0, 0, *destination));
                }
                if let Ok(value) = i8::try_from(*value) {
                    code.extend_from_slice(&[0x83, 0xF8 | (*destination & 7), value as u8]);
                } else {
                    code.extend_from_slice(&[0x81, 0xF8 | (*destination & 7)]);
                    code.extend_from_slice(&value.to_le_bytes());
                }
            }
            Item::SetConditionByte {
                condition,
                destination,
            } => {
                if *destination >= 4 {
                    code.push(rex(false, 0, 0, *destination));
                }
                code.extend_from_slice(&[
                    0x0F,
                    match condition {
                        BranchCondition::Equal => 0x94,
                        BranchCondition::NotEqual => 0x95,
                        BranchCondition::Less => 0x9C,
                        BranchCondition::GreaterEqual => 0x9D,
                        BranchCondition::LessEqual => 0x9E,
                        BranchCondition::Greater => 0x9F,
                    },
                    0xC0 | (*destination & 7),
                ]);
            }
            Item::LeaRip { label, destination } => {
                let target = *labels.get(label).ok_or(ToolchainError::InvalidAssembly)?;
                let displacement = relative_displacement(target, offset + 7)?;
                code.push(rex(true, *destination, 0, 0));
                code.extend_from_slice(&[0x8D, 0x05 | ((*destination & 7) << 3)]);
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
                        BranchCondition::Less => 0x8C,
                        BranchCondition::GreaterEqual => 0x8D,
                        BranchCondition::LessEqual => 0x8E,
                        BranchCondition::Greater => 0x8F,
                    },
                ]);
                code.extend_from_slice(&displacement.to_le_bytes());
            }
            Item::RepStosByte => code.extend_from_slice(&[0xF3, 0xAA]),
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
        let mut macros = BTreeMap::new();
        macros.insert(String::from("OCT_STDIN"), String::from("0"));
        macros.insert(String::from("OCT_STDOUT"), String::from("1"));
        macros.insert(String::from("OCT_STDERR"), String::from("2"));
        Self {
            macros,
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
        if angled
            && matches!(
                name.as_str(),
                "stddef.h"
                    | "stdint.h"
                    | "stdarg.h"
                    | "stdio.h"
                    | "stdlib.h"
                    | "string.h"
                    | "unistd.h"
                    | "fcntl.h"
                    | "errno.h"
                    | "octagram.h"
                    | "spawn.h"
                    | "sys/types.h"
                    | "sys/wait.h"
                    | "sys/stat.h"
            )
        {
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

fn rex(wide: bool, reg: u8, index: u8, base: u8) -> u8 {
    0x40 | (u8::from(wide) << 3) | ((reg >> 3) << 2) | ((index >> 3) << 1) | (base >> 3)
}

fn memory_displacement_size(memory: MemoryOperand) -> (u8, usize) {
    let base = memory.base & 7;
    if memory.displacement == 0 && base != 5 {
        (0, 0)
    } else if i8::try_from(memory.displacement).is_ok() {
        (1, 1)
    } else {
        (2, 4)
    }
}

fn memory_modrm_size(memory: MemoryOperand) -> usize {
    let (_, displacement_size) = memory_displacement_size(memory);
    1 + usize::from(memory.base & 7 == 4) + displacement_size
}

fn emit_memory_modrm(output: &mut Vec<u8>, register: u8, memory: MemoryOperand) {
    let (mode, displacement_size) = memory_displacement_size(memory);
    let base = memory.base & 7;
    output.push((mode << 6) | ((register & 7) << 3) | if base == 4 { 4 } else { base });
    if base == 4 {
        output.push(0x24);
    }
    match displacement_size {
        0 => {}
        1 => output.push(memory.displacement as i8 as u8),
        _ => output.extend_from_slice(&memory.displacement.to_le_bytes()),
    }
}

fn parse_memory_operand(operand: &str) -> Result<MemoryOperand, ToolchainError> {
    let (displacement, base) = operand
        .trim()
        .split_once('(')
        .ok_or(ToolchainError::InvalidAssembly)?;
    let base = base
        .strip_suffix(')')
        .ok_or(ToolchainError::InvalidAssembly)?;
    let displacement = if displacement.trim().is_empty() {
        0
    } else {
        parse_signed_integer(displacement.trim())?
    };
    Ok(MemoryOperand {
        displacement,
        base: parse_register(base.trim())?,
    })
}

fn parse_signed_integer(value: &str) -> Result<i32, ToolchainError> {
    if let Some(value) = value.strip_prefix('-') {
        let value = parse_integer(value)?;
        let value = i64::try_from(value).map_err(|_| ToolchainError::InvalidAssembly)?;
        i32::try_from(-value).map_err(|_| ToolchainError::InvalidAssembly)
    } else {
        let value = parse_integer(value)?;
        i32::try_from(value).map_err(|_| ToolchainError::InvalidAssembly)
    }
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
                ".file" | ".loc" | ".local" | ".type" | ".size" => {}
                ".align" | ".balign" | ".p2align" => {
                    let value = parse_integer(arguments.trim())?;
                    let alignment = if directive == ".p2align" {
                        1usize
                            .checked_shl(
                                u32::try_from(value)
                                    .map_err(|_| ToolchainError::InvalidAssembly)?,
                            )
                            .ok_or(ToolchainError::InvalidAssembly)?
                    } else {
                        usize::try_from(value).map_err(|_| ToolchainError::InvalidAssembly)?
                    };
                    if alignment == 0 || alignment > 4096 {
                        return Err(ToolchainError::InvalidAssembly);
                    }
                    items.push(Item::Align(alignment));
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
                if let Some(immediate) = left.trim().strip_prefix('$') {
                    let right = right.trim();
                    if let Ok(register) = parse_byte_register(right) {
                        Item::MoveImmediateByte {
                            register,
                            value: parse_byte_immediate(immediate)?,
                        }
                    } else if let Ok(register) = parse_32bit_register(right) {
                        Item::MoveImmediate32 {
                            register,
                            value: parse_32bit_immediate(immediate)?,
                        }
                    } else {
                        Item::MoveImmediate {
                            register: parse_register(right)?,
                            value: parse_immediate(immediate)?,
                        }
                    }
                } else if parse_byte_register(left.trim()).is_ok() {
                    return Err(ToolchainError::InvalidAssembly);
                } else if let Ok(source) = parse_32bit_register(left.trim()) {
                    if let Ok(destination) = parse_32bit_register(right.trim()) {
                        Item::MoveRegister32 {
                            source,
                            destination,
                        }
                    } else {
                        Item::MoveRegister32ToMemory {
                            source,
                            memory: parse_memory_operand(right.trim())?,
                        }
                    }
                } else if let Ok(source) = parse_register(left.trim()) {
                    if let Ok(destination) = parse_register(right.trim()) {
                        Item::MoveRegister {
                            source,
                            destination,
                        }
                    } else {
                        Item::MoveRegisterToMemory {
                            source,
                            memory: parse_memory_operand(right.trim())?,
                        }
                    }
                } else if let Ok(destination) = parse_32bit_register(right.trim()) {
                    Item::MoveMemory32ToRegister {
                        memory: parse_memory_operand(left.trim())?,
                        destination,
                    }
                } else {
                    Item::MoveMemoryToRegister {
                        memory: parse_memory_operand(left.trim())?,
                        destination: parse_register(right.trim())?,
                    }
                }
            }
            "movzb" => {
                let (left, right) = arguments
                    .split_once(',')
                    .ok_or(ToolchainError::InvalidAssembly)?;
                Item::MoveZeroExtendByteToRegister {
                    source: parse_byte_register(left.trim())?,
                    destination: parse_register(right.trim())?,
                }
            }
            "push" if !arguments.is_empty() => Item::PushRegister(parse_register(arguments)?),
            "pop" if !arguments.is_empty() => Item::PopRegister(parse_register(arguments)?),
            "add" | "sub" | "imul" => {
                let (left, right) = arguments
                    .split_once(',')
                    .ok_or(ToolchainError::InvalidAssembly)?;
                if let Some(immediate) = left.trim().strip_prefix('$') {
                    let operation = match mnemonic {
                        "add" => BinaryOperation::Add,
                        "sub" => BinaryOperation::Subtract,
                        _ => return Err(ToolchainError::InvalidAssembly),
                    };
                    Item::ImmediateBinary {
                        operation,
                        value: parse_signed_integer(immediate)?,
                        destination: parse_register(right.trim())?,
                    }
                } else {
                    let operation = match mnemonic {
                        "add" => BinaryOperation::Add,
                        "sub" => BinaryOperation::Subtract,
                        _ => BinaryOperation::Multiply,
                    };
                    if let (Ok(source), Ok(destination)) = (
                        parse_32bit_register(left.trim()),
                        parse_32bit_register(right.trim()),
                    ) {
                        Item::BinaryRegister32 {
                            operation,
                            source,
                            destination,
                        }
                    } else {
                        Item::BinaryRegister {
                            operation,
                            source: parse_register(left.trim())?,
                            destination: parse_register(right.trim())?,
                        }
                    }
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
            "cmp" => {
                let (left, right) = arguments
                    .split_once(',')
                    .ok_or(ToolchainError::InvalidAssembly)?;
                if let Some(immediate) = left.trim().strip_prefix('$') {
                    Item::CompareImmediate32 {
                        value: parse_signed_integer(immediate)?,
                        destination: parse_32bit_register(right.trim())?,
                    }
                } else if let (Ok(left), Ok(right)) = (
                    parse_32bit_register(left.trim()),
                    parse_32bit_register(right.trim()),
                ) {
                    Item::CompareRegister32 { left, right }
                } else {
                    Item::CompareRegister {
                        left: parse_register(left.trim())?,
                        right: parse_register(right.trim())?,
                    }
                }
            }
            "sete" | "setz" | "setne" | "setnz" | "setl" | "setle" | "setg" | "setge" => {
                Item::SetConditionByte {
                    condition: match mnemonic {
                        "sete" | "setz" => BranchCondition::Equal,
                        "setne" | "setnz" => BranchCondition::NotEqual,
                        "setl" => BranchCondition::Less,
                        "setle" => BranchCondition::LessEqual,
                        "setg" => BranchCondition::Greater,
                        _ => BranchCondition::GreaterEqual,
                    },
                    destination: parse_byte_register(arguments)?,
                }
            }
            "lea" => {
                let (left, right) = arguments
                    .split_once(',')
                    .ok_or(ToolchainError::InvalidAssembly)?;
                let destination = parse_register(right.trim())?;
                let source = left.trim();
                if let Some(label) = source.strip_suffix("(%rip)") {
                    let label = label.trim();
                    if !valid_label(label) {
                        return Err(ToolchainError::InvalidAssembly);
                    }
                    Item::LeaRip {
                        label: String::from(label),
                        destination,
                    }
                } else {
                    Item::LeaMemory {
                        memory: parse_memory_operand(source)?,
                        destination,
                    }
                }
            }
            "movsxd" => {
                let (left, right) = arguments
                    .split_once(',')
                    .ok_or(ToolchainError::InvalidAssembly)?;
                Item::SignExtend32MemoryToRegister {
                    memory: parse_memory_operand(left.trim())?,
                    destination: parse_register(right.trim())?,
                }
            }
            "rep" if arguments == "stosb" => Item::RepStosByte,
            "call" | "jmp" => {
                if !valid_label(arguments) {
                    return Err(ToolchainError::InvalidAssembly);
                }
                Item::RelativeBranch {
                    label: String::from(arguments),
                    call: mnemonic == "call",
                }
            }
            "je" | "jz" | "jne" | "jnz" | "jl" | "jle" | "jg" | "jge" => {
                if !valid_label(arguments) {
                    return Err(ToolchainError::InvalidAssembly);
                }
                Item::ConditionalBranch {
                    label: String::from(arguments),
                    condition: match mnemonic {
                        "je" | "jz" => BranchCondition::Equal,
                        "jne" | "jnz" => BranchCondition::NotEqual,
                        "jl" => BranchCondition::Less,
                        "jle" => BranchCondition::LessEqual,
                        "jg" => BranchCondition::Greater,
                        _ => BranchCondition::GreaterEqual,
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
        "%rcx" => Ok(1),
        "%rdx" => Ok(2),
        "%rbx" => Ok(3),
        "%rsp" => Ok(4),
        "%rbp" => Ok(5),
        "%rsi" => Ok(6),
        "%rdi" => Ok(7),
        "%r8" => Ok(8),
        "%r9" => Ok(9),
        "%r10" => Ok(10),
        "%r11" => Ok(11),
        "%r12" => Ok(12),
        "%r13" => Ok(13),
        "%r14" => Ok(14),
        "%r15" => Ok(15),
        _ => Err(ToolchainError::InvalidAssembly),
    }
}

fn parse_32bit_register(register: &str) -> Result<u8, ToolchainError> {
    match register {
        "%eax" => Ok(0),
        "%ecx" => Ok(1),
        "%edx" => Ok(2),
        "%ebx" => Ok(3),
        "%esp" => Ok(4),
        "%ebp" => Ok(5),
        "%esi" => Ok(6),
        "%edi" => Ok(7),
        "%r8d" => Ok(8),
        "%r9d" => Ok(9),
        "%r10d" => Ok(10),
        "%r11d" => Ok(11),
        "%r12d" => Ok(12),
        "%r13d" => Ok(13),
        "%r14d" => Ok(14),
        "%r15d" => Ok(15),
        _ => Err(ToolchainError::InvalidAssembly),
    }
}

fn parse_byte_register(register: &str) -> Result<u8, ToolchainError> {
    match register {
        "%al" => Ok(0),
        "%cl" => Ok(1),
        "%dl" => Ok(2),
        "%bl" => Ok(3),
        "%spl" => Ok(4),
        "%bpl" => Ok(5),
        "%sil" => Ok(6),
        "%dil" => Ok(7),
        "%r8b" => Ok(8),
        "%r9b" => Ok(9),
        "%r10b" => Ok(10),
        "%r11b" => Ok(11),
        "%r12b" => Ok(12),
        "%r13b" => Ok(13),
        "%r14b" => Ok(14),
        "%r15b" => Ok(15),
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

fn parse_immediate(value: &str) -> Result<u64, ToolchainError> {
    if let Ok(value) = value.parse::<i64>() {
        return Ok(value as u64);
    }
    if let Some(hex) = value.strip_prefix("-0x") {
        let magnitude = i64::try_from(
            u64::from_str_radix(hex, 16).map_err(|_| ToolchainError::InvalidAssembly)?,
        )
        .map_err(|_| ToolchainError::InvalidAssembly)?;
        return magnitude
            .checked_neg()
            .map(|value| value as u64)
            .ok_or(ToolchainError::InvalidAssembly);
    }
    parse_integer(value)
}

fn parse_32bit_immediate(value: &str) -> Result<u32, ToolchainError> {
    if value.starts_with('-') {
        return parse_signed_integer(value).map(|value| value as u32);
    }
    u32::try_from(parse_integer(value)?).map_err(|_| ToolchainError::InvalidAssembly)
}

fn parse_byte_immediate(value: &str) -> Result<u8, ToolchainError> {
    if value.starts_with('-') {
        let value = parse_signed_integer(value)?;
        return i8::try_from(value)
            .map(|value| value as u8)
            .map_err(|_| ToolchainError::InvalidAssembly);
    }
    u8::try_from(parse_integer(value)?).map_err(|_| ToolchainError::InvalidAssembly)
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
    LogicalNot(Box<Self>),
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
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    LogicalAnd,
    LogicalOr,
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
    For {
        initializer: Option<Box<CStatement>>,
        condition: Option<CExpression>,
        increment: Option<Box<CStatement>>,
        body: Vec<CStatement>,
    },
    Switch {
        expression: CExpression,
        cases: Vec<CSwitchCase>,
    },
    Break,
    Continue,
}

struct CSwitchCase {
    value: Option<u64>,
    body: Vec<CStatement>,
}

#[derive(Clone)]
struct CControlTarget {
    break_label: String,
    continue_label: Option<String>,
}

impl CExpression {
    fn emit(&self, assembly: &mut String, next_label: &mut usize) {
        match self {
            Self::Number(value) => assembly.push_str(&format!("mov ${value}, %rax\n")),
            Self::Register(register) => {
                assembly.push_str(&format!("mov {}, %rax\n", register.name()));
            }
            Self::Negate(expression) => {
                expression.emit(assembly, next_label);
                assembly.push_str("push %rax\nmov $0, %rax\npop %rdi\nsub %rdi, %rax\n");
            }
            Self::LogicalNot(expression) => {
                let label = *next_label;
                *next_label += 1;
                expression.emit(assembly, next_label);
                assembly.push_str(&format!(
                    "test %rax, %rax\nmov $0, %rax\nje .Lnot_true{label}\njmp .Lnot_end{label}\n.Lnot_true{label}:\nmov $1, %rax\n.Lnot_end{label}:\n"
                ));
            }
            Self::Binary {
                operator,
                left,
                right,
            } => match operator {
                COperator::LogicalAnd | COperator::LogicalOr => {
                    let label = *next_label;
                    *next_label += 1;
                    left.emit(assembly, next_label);
                    assembly.push_str("test %rax, %rax\n");
                    if matches!(operator, COperator::LogicalAnd) {
                        assembly.push_str(&format!("je .Llogic_false{label}\n"));
                    } else {
                        assembly.push_str(&format!("jne .Llogic_true{label}\n"));
                    }
                    right.emit(assembly, next_label);
                    assembly.push_str("test %rax, %rax\n");
                    if matches!(operator, COperator::LogicalAnd) {
                        assembly.push_str(&format!("je .Llogic_false{label}\n"));
                        assembly.push_str(&format!(
                                "mov $1, %rax\njmp .Llogic_end{label}\n.Llogic_false{label}:\nmov $0, %rax\n.Llogic_end{label}:\n"
                            ));
                    } else {
                        assembly.push_str(&format!("jne .Llogic_true{label}\n"));
                        assembly.push_str(&format!(
                                "mov $0, %rax\njmp .Llogic_end{label}\n.Llogic_true{label}:\nmov $1, %rax\n.Llogic_end{label}:\n"
                            ));
                    }
                }
                COperator::Equal
                | COperator::NotEqual
                | COperator::Less
                | COperator::LessEqual
                | COperator::Greater
                | COperator::GreaterEqual => {
                    let label = *next_label;
                    *next_label += 1;
                    left.emit(assembly, next_label);
                    assembly.push_str("push %rax\n");
                    right.emit(assembly, next_label);
                    assembly.push_str("pop %rdi\ncmp %rax, %rdi\nmov $0, %rax\n");
                    let branch = match operator {
                        COperator::Equal => "je",
                        COperator::NotEqual => "jne",
                        COperator::Less => "jl",
                        COperator::LessEqual => "jle",
                        COperator::Greater => "jg",
                        COperator::GreaterEqual => "jge",
                        _ => unreachable!(),
                    };
                    assembly.push_str(&format!(
                            "{branch} .Lcompare_true{label}\njmp .Lcompare_end{label}\n.Lcompare_true{label}:\nmov $1, %rax\n.Lcompare_end{label}:\n"
                        ));
                }
                COperator::Add | COperator::Subtract | COperator::Multiply => {
                    left.emit(assembly, next_label);
                    assembly.push_str("push %rax\n");
                    right.emit(assembly, next_label);
                    assembly.push_str("pop %rdi\n");
                    match operator {
                        COperator::Add => assembly.push_str("add %rdi, %rax\n"),
                        COperator::Subtract => {
                            assembly.push_str("sub %rax, %rdi\nmov %rdi, %rax\n");
                        }
                        COperator::Multiply => assembly.push_str("imul %rdi, %rax\n"),
                        _ => unreachable!(),
                    }
                }
            },
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
        emit_statements(
            &self.statements,
            &mut assembly,
            &mut next_label,
            &mut Vec::new(),
        );
        self.return_expression.emit(&mut assembly, &mut next_label);
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

fn emit_statements(
    statements: &[CStatement],
    assembly: &mut String,
    next_label: &mut usize,
    controls: &mut Vec<CControlTarget>,
) {
    for statement in statements {
        emit_statement(statement, assembly, next_label, controls);
    }
}

fn emit_statement(
    statement: &CStatement,
    assembly: &mut String,
    next_label: &mut usize,
    controls: &mut Vec<CControlTarget>,
) {
    match statement {
        CStatement::Declare {
            register,
            expression,
        }
        | CStatement::Assign {
            register,
            expression,
        } => {
            expression.emit(assembly, next_label);
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
            condition.emit(assembly, next_label);
            assembly.push_str(&format!("test %rax, %rax\nje .Lif_else{label}\n"));
            emit_statements(then_body, assembly, next_label, controls);
            if !else_body.is_empty() {
                assembly.push_str(&format!("jmp .Lif_end{label}\n"));
            }
            assembly.push_str(&format!(".Lif_else{label}:\n"));
            emit_statements(else_body, assembly, next_label, controls);
            if !else_body.is_empty() {
                assembly.push_str(&format!(".Lif_end{label}:\n"));
            }
        }
        CStatement::While { condition, body } => {
            let label = *next_label;
            *next_label += 1;
            let start_label = format!(".Lwhile_start{label}");
            let end_label = format!(".Lwhile_end{label}");
            assembly.push_str(&format!(".Lwhile_start{label}:\n"));
            condition.emit(assembly, next_label);
            assembly.push_str(&format!("test %rax, %rax\nje .Lwhile_end{label}\n"));
            controls.push(CControlTarget {
                break_label: end_label,
                continue_label: Some(start_label.clone()),
            });
            emit_statements(body, assembly, next_label, controls);
            controls.pop();
            assembly.push_str(&format!("jmp .Lwhile_start{label}\n.Lwhile_end{label}:\n"));
        }
        CStatement::For {
            initializer,
            condition,
            increment,
            body,
        } => {
            let label = *next_label;
            *next_label += 1;
            if let Some(initializer) = initializer {
                emit_statement(initializer, assembly, next_label, controls);
            }
            let start_label = format!(".Lfor_start{label}");
            let increment_label = format!(".Lfor_increment{label}");
            let end_label = format!(".Lfor_end{label}");
            assembly.push_str(&format!("{start_label}:\n"));
            if let Some(condition) = condition {
                condition.emit(assembly, next_label);
                assembly.push_str(&format!("test %rax, %rax\nje {end_label}\n"));
            }
            controls.push(CControlTarget {
                break_label: end_label.clone(),
                continue_label: Some(increment_label.clone()),
            });
            emit_statements(body, assembly, next_label, controls);
            controls.pop();
            assembly.push_str(&format!("{increment_label}:\n"));
            if let Some(increment) = increment {
                emit_statement(increment, assembly, next_label, controls);
            }
            assembly.push_str(&format!("jmp {start_label}\n{end_label}:\n"));
        }
        CStatement::Switch { expression, cases } => {
            let label = *next_label;
            *next_label += 1;
            let end_label = format!(".Lswitch_end{label}");
            expression.emit(assembly, next_label);
            for (index, case) in cases.iter().enumerate() {
                if let Some(value) = case.value {
                    assembly.push_str(&format!(
                        "mov ${value}, %rdi\ncmp %rdi, %rax\nje .Lswitch_case{label}_{index}\n"
                    ));
                }
            }
            let default_index = cases.iter().position(|case| case.value.is_none());
            if let Some(index) = default_index {
                assembly.push_str(&format!("jmp .Lswitch_case{label}_{index}\n"));
            } else {
                assembly.push_str(&format!("jmp {end_label}\n"));
            }
            controls.push(CControlTarget {
                break_label: end_label.clone(),
                continue_label: None,
            });
            for (index, case) in cases.iter().enumerate() {
                assembly.push_str(&format!(".Lswitch_case{label}_{index}:\n"));
                emit_statements(&case.body, assembly, next_label, controls);
            }
            controls.pop();
            assembly.push_str(&format!("{end_label}:\n"));
        }
        CStatement::Break => {
            if let Some(target) = controls.last() {
                assembly.push_str(&format!("jmp {}\n", target.break_label));
            }
        }
        CStatement::Continue => {
            if let Some(target) = controls
                .iter()
                .rev()
                .find_map(|target| target.continue_label.as_deref())
            {
                assembly.push_str(&format!("jmp {target}\n"));
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
    controls: Vec<bool>,
}

impl<'a> CParser<'a> {
    fn new(source: &str, macros: &'a BTreeMap<String, String>) -> Result<Self, ToolchainError> {
        Ok(Self {
            tokens: tokenize_c(source)?,
            index: 0,
            macros,
            variables: BTreeMap::new(),
            local_registers: Vec::new(),
            controls: Vec::new(),
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
            self.controls.push(true);
            let body = self.parse_block(control_depth + 1, strings)?;
            self.controls.pop();
            return Ok(CStatement::While { condition, body });
        }
        if self.peek_identifier("for") {
            if control_depth >= MAX_C_CONTROL_DEPTH {
                return Err(ToolchainError::UnsupportedC);
            }
            self.index += 1;
            self.expect_symbol(b'(')?;
            let initializer = if self.peek_symbol(b';') {
                None
            } else if self.peek_identifier("int") {
                self.index += 1;
                let Some(CToken::Identifier(name)) = self.next_token() else {
                    return Err(ToolchainError::InvalidCSource);
                };
                if self.variables.contains_key(&name) {
                    return Err(ToolchainError::InvalidCSource);
                }
                self.expect_symbol(b'=')?;
                let expression = self.parse_expression(0)?;
                let register = self.allocate_local(name)?;
                Some(Box::new(CStatement::Declare {
                    register,
                    expression,
                }))
            } else {
                Some(Box::new(self.parse_for_assignment()?))
            };
            self.expect_symbol(b';')?;
            let condition = if self.peek_symbol(b';') {
                None
            } else {
                Some(self.parse_expression(0)?)
            };
            self.expect_symbol(b';')?;
            let increment = if self.peek_symbol(b')') {
                None
            } else {
                Some(Box::new(self.parse_for_assignment()?))
            };
            self.expect_symbol(b')')?;
            self.controls.push(true);
            let body = self.parse_block(control_depth + 1, strings)?;
            self.controls.pop();
            return Ok(CStatement::For {
                initializer,
                condition,
                increment,
                body,
            });
        }
        if self.peek_identifier("switch") {
            if control_depth >= MAX_C_CONTROL_DEPTH {
                return Err(ToolchainError::UnsupportedC);
            }
            self.index += 1;
            self.expect_symbol(b'(')?;
            let expression = self.parse_expression(0)?;
            self.expect_symbol(b')')?;
            self.expect_symbol(b'{')?;
            self.controls.push(false);
            let mut cases: Vec<CSwitchCase> = Vec::new();
            while !self.peek_symbol(b'}') {
                let value = if self.peek_identifier("case") {
                    self.index += 1;
                    let value = self.expect_number()?;
                    if cases.iter().any(|case| case.value == Some(value)) {
                        return Err(ToolchainError::InvalidCSource);
                    }
                    Some(value)
                } else if self.peek_identifier("default") {
                    self.index += 1;
                    if cases.iter().any(|case| case.value.is_none()) {
                        return Err(ToolchainError::InvalidCSource);
                    }
                    None
                } else {
                    return Err(ToolchainError::InvalidCSource);
                };
                self.expect_symbol(b':')?;
                let mut body = Vec::new();
                while !self.peek_symbol(b'}')
                    && !self.peek_identifier("case")
                    && !self.peek_identifier("default")
                {
                    if self.peek_identifier("return") {
                        return Err(ToolchainError::UnsupportedC);
                    }
                    body.push(self.parse_statement(control_depth + 1, strings)?);
                }
                cases.push(CSwitchCase { value, body });
            }
            self.expect_symbol(b'}')?;
            self.controls.pop();
            return Ok(CStatement::Switch { expression, cases });
        }
        if self.peek_identifier("break") {
            self.index += 1;
            self.expect_symbol(b';')?;
            if self.controls.is_empty() {
                return Err(ToolchainError::InvalidCSource);
            }
            return Ok(CStatement::Break);
        }
        if self.peek_identifier("continue") {
            self.index += 1;
            self.expect_symbol(b';')?;
            if !self.controls.iter().any(|is_loop| *is_loop) {
                return Err(ToolchainError::InvalidCSource);
            }
            return Ok(CStatement::Continue);
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
            let register = self.allocate_local(name)?;
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

        let writer = if self.peek_identifier("oct_write") {
            self.index += 1;
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
            (message, length)
        } else if self.peek_identifier("puts") || self.peek_identifier("printf") {
            let is_puts = self.peek_identifier("puts");
            self.index += 1;
            self.expect_symbol(b'(')?;
            let mut message = self.expect_string()?;
            if is_puts {
                message.push(b'\n');
            } else if message.contains(&b'%') {
                return Err(ToolchainError::UnsupportedC);
            }
            self.expect_symbol(b')')?;
            let length = message.len();
            (message, length)
        } else {
            return Err(ToolchainError::UnsupportedC);
        };
        self.expect_symbol(b';')?;
        let string_index = strings.len();
        strings.push(writer.0);
        Ok(CStatement::Write {
            string_index,
            length: writer.1,
            saved_registers: self.local_registers.clone(),
        })
    }

    fn allocate_local(&mut self, name: String) -> Result<CRegister, ToolchainError> {
        let register = match self.variables.len() {
            0 => CRegister::Rsi,
            1 => CRegister::Rdx,
            _ => return Err(ToolchainError::UnsupportedC),
        };
        self.variables.insert(name, register);
        self.local_registers.push(register);
        Ok(register)
    }

    fn parse_for_assignment(&mut self) -> Result<CStatement, ToolchainError> {
        let Some(register) = self.peek_assignment() else {
            return Err(ToolchainError::UnsupportedC);
        };
        self.index += 2;
        let expression = self.parse_expression(0)?;
        Ok(CStatement::Assign {
            register,
            expression,
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
                    7,
                    depth + 1,
                    operations,
                )?))
            }
            Some(CToken::Symbol(b'+')) => {
                *operations += 1;
                if *operations > MAX_C_EXPRESSION_OPERATIONS {
                    return Err(ToolchainError::UnsupportedC);
                }
                self.parse_expression_nested(7, depth + 1, operations)?
            }
            Some(CToken::Symbol(b'!')) => {
                *operations += 1;
                if *operations > MAX_C_EXPRESSION_OPERATIONS {
                    return Err(ToolchainError::UnsupportedC);
                }
                CExpression::LogicalNot(Box::new(self.parse_expression_nested(
                    7,
                    depth + 1,
                    operations,
                )?))
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
            let double_symbol = matches!(
                self.tokens.get(self.index + 1),
                Some(CToken::Symbol(next)) if *next == *symbol
            );
            let second_is_equal =
                matches!(self.tokens.get(self.index + 1), Some(CToken::Symbol(b'=')));
            let (precedence, operator, token_count) = match *symbol {
                b'|' if double_symbol => (1, COperator::LogicalOr, 2),
                b'&' if double_symbol => (2, COperator::LogicalAnd, 2),
                b'=' if second_is_equal => (3, COperator::Equal, 2),
                b'!' if second_is_equal => (3, COperator::NotEqual, 2),
                b'<' if second_is_equal => (4, COperator::LessEqual, 2),
                b'>' if second_is_equal => (4, COperator::GreaterEqual, 2),
                b'<' => (4, COperator::Less, 1),
                b'>' => (4, COperator::Greater, 1),
                b'+' => (5, COperator::Add, 1),
                b'-' => (5, COperator::Subtract, 1),
                b'*' => (6, COperator::Multiply, 1),
                _ => break,
            };
            if precedence < minimum_precedence {
                break;
            }
            self.index += token_count;
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
        if matches!(self.tokens.get(self.index + 2), Some(CToken::Symbol(b'='))) {
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
            symbol @ (b'(' | b')' | b'{' | b'}' | b',' | b';' | b'+' | b'-' | b'*' | b'='
            | b':' | b'!' | b'<' | b'>' | b'&' | b'|') => {
                tokens.push(CToken::Symbol(symbol));
                index += 1;
            }
            _ => return Err(ToolchainError::UnsupportedC),
        }
    }
    Ok(tokens)
}

// --- ELF64 relocatable object linker ---------------------------------------

const ELF_SHT_PROGBITS: u32 = 1;
const ELF_SHT_SYMTAB: u32 = 2;
const ELF_SHT_STRTAB: u32 = 3;
const ELF_SHT_RELA: u32 = 4;
const ELF_SHT_NOBITS: u32 = 8;
const ELF_SHF_ALLOC: u64 = 0x2;
const ELF_SHF_EXECINSTR: u64 = 0x4;
const ELF_SHN_UNDEF: u16 = 0;
const ELF_SHN_ABS: u16 = 0xfff1;
const ELF_SHN_COMMON: u16 = 0xfff2;
const ELF_ET_REL: u16 = 1;
const ELF_ET_EXEC: u16 = 2;
const ELF_EM_X86_64: u16 = 62;
const ELF_PT_LOAD: u32 = 1;
const ELF_PF_R: u32 = 4;
const ELF_PF_W: u32 = 2;
const ELF_PF_X: u32 = 1;
const ELF_SYMBOL_SIZE: usize = 24;
const ELF_RELA_SIZE: usize = 24;
const MAX_ELF_OBJECT_SIZE: usize = 8 * 1024 * 1024;
const MAX_ELF_LINKED_SIZE: usize = 16 * 1024 * 1024;
const MAX_ELF_LINKED_OBJECTS: usize = 32;

#[derive(Clone, Copy)]
struct ElfSection {
    section_type: u32,
    flags: u64,
    offset: usize,
    size: usize,
    link: u32,
    info: u32,
    alignment: usize,
}

#[derive(Clone, Copy)]
enum ElfPlacement {
    Text(usize),
    Data(usize),
    Bss(usize),
}

pub fn is_elf_object(bytes: &[u8]) -> bool {
    bytes.len() >= ELF_HEADER_SIZE && bytes.get(..4) == Some(b"\x7fELF".as_slice())
}

fn elf_u16(bytes: &[u8], offset: usize) -> Result<u16, ToolchainError> {
    let field = bytes
        .get(offset..offset + 2)
        .ok_or(ToolchainError::OutOfBounds)?;
    Ok(u16::from_le_bytes([field[0], field[1]]))
}

fn elf_u32(bytes: &[u8], offset: usize) -> Result<u32, ToolchainError> {
    let field = bytes
        .get(offset..offset + 4)
        .ok_or(ToolchainError::OutOfBounds)?;
    Ok(u32::from_le_bytes([field[0], field[1], field[2], field[3]]))
}

fn elf_u64(bytes: &[u8], offset: usize) -> Result<u64, ToolchainError> {
    let field = bytes
        .get(offset..offset + 8)
        .ok_or(ToolchainError::OutOfBounds)?;
    let mut value = [0u8; 8];
    value.copy_from_slice(field);
    Ok(u64::from_le_bytes(value))
}

fn align_up(value: usize, alignment: usize) -> Result<usize, ToolchainError> {
    let alignment = alignment.max(1);
    if !alignment.is_power_of_two() {
        return Err(ToolchainError::InvalidElfObject);
    }
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .ok_or(ToolchainError::OutOfBounds)
}

/// Link one ELF64 relocatable object (`tcc -c` output) into a static OCTAGRAM
/// executable with a read-execute text segment and a read-write data segment.
///
/// Supported relocations: `R_X86_64_64`, `PC32`, `PLT32`, `32`, `32S`. Anything
/// else fails loudly instead of producing a silently broken binary.

/// Link one ELF64 relocatable object. Kept as a convenience wrapper around the
/// multi-object linker so existing callers retain their original interface.
pub fn link_elf_object(object: &[u8]) -> Result<Vec<u8>, ToolchainError> {
    link_elf_objects(&[object])
}

/// Link multiple ELF64 x86_64 relocatable objects into one static OCTAGRAM
/// executable. Global and weak symbols are resolved across object boundaries.
///
/// Supported relocations: R_X86_64_64, PC32, PLT32, 32, and 32S. GOT/PLT
/// construction, TLS relocations, COMMON symbols, and archives are deliberately
/// rejected until their ABI and extraction rules are implemented.
pub fn link_elf_objects(objects: &[&[u8]]) -> Result<Vec<u8>, ToolchainError> {
    if objects.is_empty() || objects.len() > MAX_ELF_LINKED_OBJECTS {
        return Err(ToolchainError::InvalidElfObject);
    }

    struct ParsedObject {
        sections: Vec<ElfSection>,
        placements: Vec<Option<ElfPlacement>>,
        symtab_index: usize,
        symtab: ElfSection,
        strings: ElfSection,
    }
    struct LocalSymbol {
        name: Vec<u8>,
        binding: u8,
        value: Option<u64>,
    }

    let mut parsed_objects = Vec::with_capacity(objects.len());
    let mut text = Vec::new();
    let mut data = Vec::new();
    let mut bss_size = 0usize;

    // Pass 1: validate each ET_REL object and lay out all allocated sections.
    for object in objects {
        if object.len() > MAX_ELF_OBJECT_SIZE
            || !is_elf_object(object)
            || object[4] != 2
            || object[5] != 1
            || object[6] != 1
            || elf_u16(object, 16)? != ELF_ET_REL
            || elf_u16(object, 18)? != ELF_EM_X86_64
        {
            return Err(ToolchainError::InvalidElfObject);
        }

        let section_offset = usize::try_from(elf_u64(object, 40)?)
            .map_err(|_| ToolchainError::InvalidElfObject)?;
        let section_entry_size = elf_u16(object, 58)? as usize;
        let section_count = elf_u16(object, 60)? as usize;
        let section_name_index = elf_u16(object, 62)? as usize;
        let section_table_size = section_count
            .checked_mul(ELF_HEADER_SIZE)
            .ok_or(ToolchainError::OutOfBounds)?;
        if section_count == 0
            || section_count > 96
            || section_entry_size != ELF_HEADER_SIZE
            || section_name_index >= section_count
            || section_offset
                .checked_add(section_table_size)
                .is_none_or(|end| end > object.len())
        {
            return Err(ToolchainError::InvalidElfObject);
        }

        let mut sections = Vec::with_capacity(section_count);
        for index in 0..section_count {
            let base = section_offset
                .checked_add(index * ELF_HEADER_SIZE)
                .ok_or(ToolchainError::OutOfBounds)?;
            let section = ElfSection {
                section_type: elf_u32(object, base + 4)?,
                flags: elf_u64(object, base + 8)?,
                offset: usize::try_from(elf_u64(object, base + 24)?)
                    .map_err(|_| ToolchainError::InvalidElfObject)?,
                size: usize::try_from(elf_u64(object, base + 32)?)
                    .map_err(|_| ToolchainError::InvalidElfObject)?,
                link: elf_u32(object, base + 40)?,
                info: elf_u32(object, base + 44)?,
                alignment: usize::try_from(elf_u64(object, base + 48)?)
                    .map_err(|_| ToolchainError::InvalidElfObject)?,
            };
            if section.section_type != ELF_SHT_NOBITS {
                let end = section
                    .offset
                    .checked_add(section.size)
                    .ok_or(ToolchainError::OutOfBounds)?;
                if end > object.len() {
                    return Err(ToolchainError::InvalidElfObject);
                }
            }
            sections.push(section);
        }

        let mut placements = alloc::vec![None; section_count];
        for (index, section) in sections.iter().enumerate() {
            if section.flags & ELF_SHF_ALLOC == 0 || index == 0 {
                continue;
            }
            let alignment = section.alignment.max(1);
            if section.section_type == ELF_SHT_NOBITS {
                let start = align_up(
                    data.len()
                        .checked_add(bss_size)
                        .ok_or(ToolchainError::OutOfBounds)?,
                    alignment,
                )?;
                placements[index] = Some(ElfPlacement::Bss(start));
                bss_size = start
                    .checked_add(section.size)
                    .and_then(|end| end.checked_sub(data.len()))
                    .ok_or(ToolchainError::OutOfBounds)?;
            } else if section.section_type == ELF_SHT_PROGBITS {
                let source = object
                    .get(section.offset..section.offset + section.size)
                    .ok_or(ToolchainError::InvalidElfObject)?;
                if section.flags & ELF_SHF_EXECINSTR != 0 {
                    let start = align_up(text.len(), alignment)?;
                    text.resize(start, 0);
                    text.extend_from_slice(source);
                    placements[index] = Some(ElfPlacement::Text(start));
                } else {
                    let start = align_up(data.len(), alignment)?;
                    data.resize(start, 0);
                    data.extend_from_slice(source);
                    placements[index] = Some(ElfPlacement::Data(start));
                }
            }
            let linked_size = text
                .len()
                .checked_add(data.len())
                .and_then(|size| size.checked_add(bss_size))
                .ok_or(ToolchainError::OutOfBounds)?;
            if linked_size > MAX_ELF_LINKED_SIZE {
                return Err(ToolchainError::OutOfBounds);
            }
        }

        let symtab_index = sections
            .iter()
            .position(|section| section.section_type == ELF_SHT_SYMTAB)
            .ok_or(ToolchainError::InvalidElfObject)?;
        let symtab = sections[symtab_index];
        if symtab.size % ELF_SYMBOL_SIZE != 0 || symtab.link as usize >= section_count {
            return Err(ToolchainError::InvalidElfObject);
        }
        let strings = sections[symtab.link as usize];
        if strings.section_type != ELF_SHT_STRTAB {
            return Err(ToolchainError::InvalidElfObject);
        }
        parsed_objects.push(ParsedObject {
            sections,
            placements,
            symtab_index,
            symtab,
            strings,
        });
    }

    if text.is_empty() {
        return Err(ToolchainError::InvalidElfObject);
    }

    let text_size = text.len();
    let data_vaddr = ELF_BASE
        .checked_add(align_up(text_size, 0x1000)? as u64)
        .ok_or(ToolchainError::OutOfBounds)?;
    let data_memory_size = data
        .len()
        .checked_add(bss_size)
        .ok_or(ToolchainError::OutOfBounds)?;

    // Pass 2: resolve every defined symbol to its final virtual address and
    // build the global symbol table before applying any relocation.
    let mut object_symbols: Vec<Vec<LocalSymbol>> = Vec::with_capacity(objects.len());
    let mut global_symbols: BTreeMap<Vec<u8>, (u64, bool)> = BTreeMap::new();
    let mut entry = None;

    for (object, parsed) in objects.iter().zip(parsed_objects.iter()) {
        let symbol_count = parsed.symtab.size / ELF_SYMBOL_SIZE;
        let mut symbols = Vec::with_capacity(symbol_count);
        for symbol_index in 0..symbol_count {
            let base = parsed
                .symtab
                .offset
                .checked_add(symbol_index * ELF_SYMBOL_SIZE)
                .ok_or(ToolchainError::OutOfBounds)?;
            let name_offset = elf_u32(object, base)? as usize;
            let info = *object
                .get(base + 4)
                .ok_or(ToolchainError::InvalidElfObject)?;
            let binding = info >> 4;
            if binding > 2 {
                return Err(ToolchainError::InvalidElfObject);
            }
            let section_index = elf_u16(object, base + 6)?;
            let symbol_value = elf_u64(object, base + 8)?;
            let name_start = parsed
                .strings
                .offset
                .checked_add(name_offset)
                .ok_or(ToolchainError::OutOfBounds)?;
            let strings_end = parsed
                .strings
                .offset
                .checked_add(parsed.strings.size)
                .ok_or(ToolchainError::OutOfBounds)?;
            if name_offset >= parsed.strings.size {
                return Err(ToolchainError::InvalidElfObject);
            }
            let name_tail = object
                .get(name_start..strings_end)
                .ok_or(ToolchainError::InvalidElfObject)?;
            let name_length = name_tail
                .iter()
                .position(|byte| *byte == 0)
                .ok_or(ToolchainError::InvalidElfObject)?;
            let name = name_tail[..name_length].to_vec();

            let value = match section_index {
                ELF_SHN_UNDEF => None,
                ELF_SHN_ABS => Some(symbol_value),
                ELF_SHN_COMMON => return Err(ToolchainError::UnsupportedCommonSymbol),
                index => {
                    let section_index = index as usize;
                    let section = parsed
                        .sections
                        .get(section_index)
                        .ok_or(ToolchainError::InvalidElfObject)?;
                    let placement = parsed
                        .placements
                        .get(section_index)
                        .copied()
                        .flatten()
                        .ok_or(ToolchainError::InvalidElfObject)?;
                    if symbol_value > section.size as u64 {
                        return Err(ToolchainError::InvalidElfObject);
                    }
                    let relative = symbol_value;
                    Some(match placement {
                        ElfPlacement::Text(start) => ELF_BASE
                            .checked_add(start as u64)
                            .and_then(|base| base.checked_add(relative))
                            .ok_or(ToolchainError::OutOfBounds)?,
                        ElfPlacement::Data(start) => data_vaddr
                            .checked_add(start as u64)
                            .and_then(|base| base.checked_add(relative))
                            .ok_or(ToolchainError::OutOfBounds)?,
                        ElfPlacement::Bss(start) => data_vaddr
                            .checked_add(start as u64)
                            .and_then(|base| base.checked_add(relative))
                            .ok_or(ToolchainError::OutOfBounds)?,
                    })
                }
            };

            if name.as_slice() == b"_start" {
                if let Some(value) = value {
                    if entry.replace(value).is_some() {
                        return Err(ToolchainError::DuplicateSymbol);
                    }
                }
            }

            if binding != 0
                && !name.is_empty()
                && let Some(value) = value
            {
                let is_weak = binding == 2;
                match global_symbols.get(&name).copied() {
                    None => {
                        global_symbols.insert(name.clone(), (value, is_weak));
                    }
                    Some((_existing, true)) if !is_weak => {
                        global_symbols.insert(name.clone(), (value, false));
                    }
                    Some((_existing, false)) if is_weak => {}
                    Some((_existing, true)) if is_weak => {}
                    Some((_existing, false)) if !is_weak => {
                        return Err(ToolchainError::DuplicateSymbol);
                    }
                }
            }

            symbols.push(LocalSymbol {
                name,
                binding,
                value,
            });
        }
        object_symbols.push(symbols);
    }

    // Pass 3: apply RELA entries after all inter-object references can resolve.
    for (object_index, (object, parsed)) in
        objects.iter().zip(parsed_objects.iter()).enumerate()
    {
        let symbols = &object_symbols[object_index];
        for section in &parsed.sections {
            if section.section_type != ELF_SHT_RELA
                || section.link as usize != parsed.symtab_index
            {
                continue;
            }
            let target_index = section.info as usize;
            let Some(placement) = parsed.placements.get(target_index).copied().flatten() else {
                // Relocations against non-allocated metadata (for example
                // .eh_frame) are intentionally omitted from the final image.
                continue;
            };
            if matches!(placement, ElfPlacement::Bss(_))
                || section.size % ELF_RELA_SIZE != 0
            {
                return Err(ToolchainError::InvalidElfObject);
            }
            let target_size = parsed.sections[target_index].size;
            let count = section.size / ELF_RELA_SIZE;
            for relocation in 0..count {
                let base = section
                    .offset
                    .checked_add(relocation * ELF_RELA_SIZE)
                    .ok_or(ToolchainError::OutOfBounds)?;
                let relocation_offset = usize::try_from(elf_u64(object, base)?)
                    .map_err(|_| ToolchainError::InvalidElfObject)?;
                let info = elf_u64(object, base + 8)?;
                let addend = elf_u64(object, base + 16)? as i64;
                let kind = info as u32;
                if kind == 0 {
                    continue;
                }
                let symbol_index = usize::try_from(info >> 32)
                    .map_err(|_| ToolchainError::InvalidElfObject)?;
                let symbol = symbols
                    .get(symbol_index)
                    .ok_or(ToolchainError::InvalidElfObject)?;
                let symbol_value = if symbol.binding == 0 {
                    symbol.value.ok_or(ToolchainError::UndefinedSymbol)?
                } else if let Some((value, _)) = global_symbols.get(&symbol.name) {
                    *value
                } else if symbol.binding == 2 {
                    0
                } else {
                    return Err(ToolchainError::UndefinedSymbol);
                };

                let relocation_size = match kind {
                    1 => 8,
                    2 | 4 | 10 | 11 => 4,
                    _ => return Err(ToolchainError::UnsupportedRelocation),
                };
                if relocation_offset
                    .checked_add(relocation_size)
                    .is_none_or(|end| end > target_size)
                {
                    return Err(ToolchainError::OutOfBounds);
                }

                match placement {
                    ElfPlacement::Text(start) => {
                        let position = start
                            .checked_add(relocation_offset)
                            .ok_or(ToolchainError::OutOfBounds)?;
                        let place = ELF_BASE
                            .checked_add(position as u64)
                            .ok_or(ToolchainError::OutOfBounds)?;
                        apply_elf_relocation(
                            &mut text,
                            position,
                            kind,
                            symbol_value,
                            addend,
                            place,
                        )?;
                    }
                    ElfPlacement::Data(start) => {
                        let position = start
                            .checked_add(relocation_offset)
                            .ok_or(ToolchainError::OutOfBounds)?;
                        let place = data_vaddr
                            .checked_add(position as u64)
                            .ok_or(ToolchainError::OutOfBounds)?;
                        apply_elf_relocation(
                            &mut data,
                            position,
                            kind,
                            symbol_value,
                            addend,
                            place,
                        )?;
                    }
                    ElfPlacement::Bss(_) => return Err(ToolchainError::InvalidElfObject),
                }
            }
        }
    }

    let entry = entry.ok_or(ToolchainError::NoEntryPoint)?;
    if entry < ELF_BASE || entry >= ELF_BASE + text_size as u64 {
        return Err(ToolchainError::InvalidElfObject);
    }

    let segment_count = if data_memory_size == 0 { 1 } else { 2 };
    let data_offset = align_up(
        ELF_LOAD_OFFSET
            .checked_add(text_size)
            .ok_or(ToolchainError::OutOfBounds)?,
        0x1000,
    )?;
    let file_size = if segment_count == 2 {
        data_offset
            .checked_add(data.len())
            .ok_or(ToolchainError::OutOfBounds)?
    } else {
        ELF_LOAD_OFFSET
            .checked_add(text_size)
            .ok_or(ToolchainError::OutOfBounds)?
    };
    if file_size > MAX_ELF_LINKED_SIZE {
        return Err(ToolchainError::OutOfBounds);
    }

    let mut executable = Vec::with_capacity(file_size);
    executable.resize(ELF_LOAD_OFFSET, 0);
    executable.extend_from_slice(&text);
    if segment_count == 2 {
        executable.resize(data_offset, 0);
        executable.extend_from_slice(&data);
    }

    executable[..4].copy_from_slice(b"\x7fELF");
    executable[4] = 2;
    executable[5] = 1;
    executable[6] = 1;
    put_u16(&mut executable, 16, ELF_ET_EXEC);
    put_u16(&mut executable, 18, ELF_EM_X86_64);
    put_u32(&mut executable, 20, 1);
    put_u64(&mut executable, 24, entry);
    put_u64(&mut executable, 32, ELF_HEADER_SIZE as u64);
    put_u16(&mut executable, 52, ELF_HEADER_SIZE as u16);
    put_u16(&mut executable, 54, PROGRAM_HEADER_SIZE as u16);
    put_u16(&mut executable, 56, segment_count as u16);

    let text_header = ELF_HEADER_SIZE;
    put_u32(&mut executable, text_header, ELF_PT_LOAD);
    put_u32(&mut executable, text_header + 4, ELF_PF_R | ELF_PF_X);
    put_u64(&mut executable, text_header + 8, ELF_LOAD_OFFSET as u64);
    put_u64(&mut executable, text_header + 16, ELF_BASE);
    put_u64(&mut executable, text_header + 24, ELF_BASE);
    put_u64(&mut executable, text_header + 32, text_size as u64);
    put_u64(&mut executable, text_header + 40, text_size as u64);
    put_u64(&mut executable, text_header + 48, 0x1000);

    if segment_count == 2 {
        let data_header = ELF_HEADER_SIZE + PROGRAM_HEADER_SIZE;
        put_u32(&mut executable, data_header, ELF_PT_LOAD);
        put_u32(&mut executable, data_header + 4, ELF_PF_R | ELF_PF_W);
        put_u64(&mut executable, data_header + 8, data_offset as u64);
        put_u64(&mut executable, data_header + 16, data_vaddr);
        put_u64(&mut executable, data_header + 24, data_vaddr);
        put_u64(&mut executable, data_header + 32, data.len() as u64);
        put_u64(&mut executable, data_header + 40, data_memory_size as u64);
        put_u64(&mut executable, data_header + 48, 0x1000);
    }
    Ok(executable)
}

fn symbol_name_is(object: &[u8], strings: ElfSection, offset: usize, expected: &[u8]) -> bool {
    let start = strings.offset.saturating_add(offset);
    let end = strings.offset.saturating_add(strings.size);
    if offset >= strings.size || start >= object.len() {
        return false;
    }
    let limit = end.min(object.len());
    let mut index = start;
    while index < limit && object[index] != 0 {
        index += 1;
    }
    &object[start..index] == expected
}

fn apply_elf_relocation(
    buffer: &mut [u8],
    offset: usize,
    kind: u32,
    symbol: u64,
    addend: i64,
    place: u64,
) -> Result<(), ToolchainError> {
    match kind {
        1 => {
            let value = (symbol as i64).wrapping_add(addend) as u64;
            let slot = buffer
                .get_mut(offset..offset + 8)
                .ok_or(ToolchainError::OutOfBounds)?;
            slot.copy_from_slice(&value.to_le_bytes());
        }
        2 | 4 => {
            let value = (symbol as i64)
                .wrapping_add(addend)
                .wrapping_sub(place as i64);
            let value = i32::try_from(value).map_err(|_| ToolchainError::RelocationOverflow)?;
            let slot = buffer
                .get_mut(offset..offset + 4)
                .ok_or(ToolchainError::OutOfBounds)?;
            slot.copy_from_slice(&value.to_le_bytes());
        }
        10 => {
            let value = (symbol as i64).wrapping_add(addend);
            let value = u32::try_from(value).map_err(|_| ToolchainError::RelocationOverflow)?;
            let slot = buffer
                .get_mut(offset..offset + 4)
                .ok_or(ToolchainError::OutOfBounds)?;
            slot.copy_from_slice(&value.to_le_bytes());
        }
        11 => {
            let value = (symbol as i64).wrapping_add(addend);
            let value = i32::try_from(value).map_err(|_| ToolchainError::RelocationOverflow)?;
            let slot = buffer
                .get_mut(offset..offset + 4)
                .ok_or(ToolchainError::OutOfBounds)?;
            slot.copy_from_slice(&value.to_le_bytes());
        }
        _ => return Err(ToolchainError::UnsupportedRelocation),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ToolchainError, assemble, compile, compile_with_includes, link, link_elf_object,
    };

    fn put_elf64(bytes: &mut [u8], offset: usize, value: u64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn put_elf32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_elf16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    /// Build a minimal ET_REL object: `_start` moves the address of `message`
    /// into edi and exits, with one R_X86_64_32 relocation against `.data`.
    fn relocatable_object() -> alloc::vec::Vec<u8> {
        const HEADER: usize = 64;
        const SECTION: usize = 64;
        const COUNT: usize = 6;
        let text: [u8; 12] = [
            0xBF, 0, 0, 0, 0, // mov $message, %edi
            0xB8, 60, 0, 0, 0, // mov $60, %eax
            0x0F, 0x05, // syscall
        ];
        let data: [u8; 8] = *b"OCTAGRAM";

        let mut object = alloc::vec![0u8; 0x400];
        object[..4].copy_from_slice(b"\x7fELF");
        object[4] = 2;
        object[5] = 1;
        object[6] = 1;
        put_elf16(&mut object, 16, 1); // ET_REL
        put_elf16(&mut object, 18, 62); // x86_64
        put_elf32(&mut object, 20, 1);
        put_elf64(&mut object, 40, 0x240);
        put_elf16(&mut object, 52, HEADER as u16);
        put_elf16(&mut object, 58, SECTION as u16);
        put_elf16(&mut object, 60, COUNT as u16);
        put_elf16(&mut object, 62, 5); // .shstrtab

        let text_offset = 0x100;
        let data_offset = 0x120;
        let symtab_offset = 0x140;
        let rela_offset = 0x200;
        let shstrtab_offset = 0x220;
        object[text_offset..text_offset + text.len()].copy_from_slice(&text);
        object[data_offset..data_offset + data.len()].copy_from_slice(&data);
        object[shstrtab_offset..shstrtab_offset + 15].copy_from_slice(b"_start\0message\0");

        // Section 0 stays all zero (SHT_NULL).
        let section = |index: usize| 0x240 + index * SECTION;
        // 1: .text
        put_elf32(&mut object, section(1), 1);
        put_elf32(&mut object, section(1) + 4, 1);
        put_elf64(&mut object, section(1) + 8, 0x6); // ALLOC | EXECINSTR
        put_elf64(&mut object, section(1) + 24, text_offset as u64);
        put_elf64(&mut object, section(1) + 32, text.len() as u64);
        put_elf64(&mut object, section(1) + 48, 16);
        // 2: .data
        put_elf32(&mut object, section(2), 7);
        put_elf32(&mut object, section(2) + 4, 1);
        put_elf64(&mut object, section(2) + 8, 0x3); // ALLOC | WRITE
        put_elf64(&mut object, section(2) + 24, data_offset as u64);
        put_elf64(&mut object, section(2) + 32, data.len() as u64);
        put_elf64(&mut object, section(2) + 48, 8);
        // 3: .rela.text (applies to section 1, symbols from section 4)
        put_elf32(&mut object, section(3), 13);
        put_elf32(&mut object, section(3) + 4, 4);
        put_elf64(&mut object, section(3) + 24, rela_offset as u64);
        put_elf64(&mut object, section(3) + 32, 24);
        put_elf32(&mut object, section(3) + 40, 4);
        put_elf32(&mut object, section(3) + 44, 1);
        put_elf64(&mut object, section(3) + 48, 8);
        put_elf64(&mut object, section(3) + 56, 24);
        // 4: .symtab (3 entries, first is the null symbol)
        put_elf32(&mut object, section(4), 24);
        put_elf32(&mut object, section(4) + 4, 2);
        put_elf64(&mut object, section(4) + 24, symtab_offset as u64);
        put_elf64(&mut object, section(4) + 32, 72);
        put_elf32(&mut object, section(4) + 40, 5);
        put_elf32(&mut object, section(4) + 44, 1);
        put_elf64(&mut object, section(4) + 48, 8);
        put_elf64(&mut object, section(4) + 56, 24);
        // 5: string table used by .symtab (also the section-name table)
        put_elf32(&mut object, section(5), 1);
        put_elf32(&mut object, section(5) + 4, 3);
        put_elf64(&mut object, section(5) + 24, shstrtab_offset as u64);
        put_elf64(&mut object, section(5) + 32, 15);
        put_elf64(&mut object, section(5) + 48, 1);

        let symbol = |index: usize| symtab_offset + index * 24;
        put_elf32(&mut object, symbol(1), 0);
        object[symbol(1) + 4] = 0x12; // STB_GLOBAL | STT_FUNC
        put_elf16(&mut object, symbol(1) + 6, 1); // .text
        put_elf32(&mut object, symbol(2), 7);
        object[symbol(2) + 4] = 0x11; // STB_GLOBAL | STT_OBJECT
        put_elf16(&mut object, symbol(2) + 6, 2); // .data
        put_elf64(&mut object, symbol(2) + 8, 0);

        put_elf64(&mut object, rela_offset, 1); // r_offset: the imm32 field
        let info = (2u64 << 32) | 10; // R_X86_64_32 to symbol 2
        put_elf64(&mut object, rela_offset + 8, info);
        put_elf64(&mut object, rela_offset + 16, 0);
        object
    }

    #[test]
    fn links_elf_relocatable_into_two_load_segments() {
        let executable = link_elf_object(&relocatable_object()).unwrap();
        assert_eq!(&executable[..4], b"\x7fELF");
        let u16_at = |offset| u16::from_le_bytes([executable[offset], executable[offset + 1]]);
        let u64_at = |offset| {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&executable[offset..offset + 8]);
            u64::from_le_bytes(bytes)
        };
        assert_eq!(u16_at(16), 2); // ET_EXEC
        assert_eq!(u16_at(56), 2); // two program headers
        assert_eq!(u64_at(24), 0x400000); // _start at the text base
        assert_eq!(u16_at(64 + 4) & 1, 1); // text is executable
        let data_vaddr = u64_at(64 + 56 + 16);
        assert_eq!(data_vaddr, 0x401000);
        let text = &executable[0x1000..];
        assert_eq!(text[0], 0xBF);
        assert_eq!(
            u32::from_le_bytes([text[1], text[2], text[3], text[4]]),
            data_vaddr as u32
        );
        assert_eq!(&executable[0x2000..0x2008], b"OCTAGRAM");
    }

    fn rename_fixture_symbol(object: &mut [u8], symbol_index: usize, name: &[u8]) {
        // The fixture's .symtab starts at 0x140 and its string table at 0x220.
        let symbol = 0x140 + symbol_index * 24;
        let name_offset = 7;
        object[symbol..symbol + 4].copy_from_slice(&(name_offset as u32).to_le_bytes());
        let strings = 0x220;
        object[strings + name_offset..strings + name_offset + name.len()]
            .copy_from_slice(name);
        object[strings + name_offset + name.len()] = 0;
        // The original fixture's string table is 15 bytes; "external" needs
        // one additional byte including its terminating NUL.
        let string_table_size_field = 0x240 + 5 * 64 + 32;
        object[string_table_size_field..string_table_size_field + 8]
            .copy_from_slice(&16u64.to_le_bytes());
    }

    #[test]
    fn links_global_symbols_across_multiple_objects() {
        let mut caller = relocatable_object();
        // Turn the caller's data symbol into an undefined external reference.
        rename_fixture_symbol(&mut caller, 2, b"external");
        caller[0x140 + 2 * 24 + 6..0x140 + 2 * 24 + 8]
            .copy_from_slice(&0u16.to_le_bytes());

        let mut provider = relocatable_object();
        // Export the provider's text symbol as "external", not a second entry.
        rename_fixture_symbol(&mut provider, 1, b"external");

        let executable = super::link_elf_objects(&[&caller, &provider]).unwrap();
        assert_eq!(&executable[..4], b"\x7fELF");
        let text_base = u64::from_le_bytes(executable[24..32].try_into().unwrap());
        assert_eq!(text_base, 0x400000);
        // The caller's immediate now points at the provider's text, which is
        // placed after the caller's 12-byte text section aligned to 16 bytes.
        let resolved = u32::from_le_bytes(executable[0x1001..0x1005].try_into().unwrap());
        assert_eq!(resolved, 0x400010);
    }

    #[test]
    fn rejects_duplicate_global_definitions_and_unresolved_symbols() {
        let object = relocatable_object();
        assert_eq!(
            super::link_elf_objects(&[&object, &object]),
            Err(ToolchainError::DuplicateSymbol)
        );

        let mut unresolved = relocatable_object();
        rename_fixture_symbol(&mut unresolved, 2, b"missing");
        unresolved[0x140 + 2 * 24 + 6..0x140 + 2 * 24 + 8]
            .copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(
            super::link_elf_objects(&[&unresolved]),
            Err(ToolchainError::UndefinedSymbol)
        );
    }

    #[test]
    fn rejects_relocatable_objects_without_an_entry_symbol() {
        let mut object = relocatable_object();
        object[0x220] = b'm'; // rename `_start` so no entry symbol remains
        assert_eq!(
            link_elf_object(&object),
            Err(ToolchainError::NoEntryPoint)
        );
    }
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
    fn assembler_accepts_metadata_and_alignment_directives() {
        let object = assemble(
            b".file 1 \"input.c\"\n.text\n.local .L..1\n.type _start, @function\n.byte 0x90\n.align 4\n.byte 0xCC\n.loc 1 4 0\n.p2align 3\n.global _start\n_start:\nret\n.size _start, .-_start\n",
        )
        .unwrap();
        assert_eq!(u64::from_le_bytes(object[8..16].try_into().unwrap()), 8);
        assert_eq!(&object[24..], &[0x90, 0, 0, 0, 0xCC, 0, 0, 0, 0xC3]);
        assert_eq!(
            assemble(b".align 0\n.global _start\n_start:\nret\n"),
            Err(ToolchainError::InvalidAssembly)
        );
        assert_eq!(
            assemble(b".p2align 13\n.global _start\n_start:\nret\n"),
            Err(ToolchainError::InvalidAssembly)
        );
    }

    #[test]
    fn assembles_and_links_a_minimal_main_function() {
        let object = assemble(
            b".file 1 \"smoke.c\"\n.local .L..1\n.data\n.type .L..1, @object\n.size .L..1, 5\n.align 1\n.L..1:\n.byte 109\n.byte 97\n.byte 105\n.byte 110\n.byte 0\n.globl main\n.text\n.type main, @function\nmain:\npush %rbp\nmov %rsp, %rbp\nsub $16, %rsp\nmov %rsp, -8(%rbp)\n.loc 1 2\nmov $42, %rax\njmp .L.return.main\nmov $0, %rax\n.L.return.main:\nmov %rbp, %rsp\npop %rbp\nret\n.size main, .-main\n",
        )
        .unwrap();
        let executable = link(&object).unwrap();
        assert_eq!(
            u64::from_le_bytes(executable[24..32].try_into().unwrap()),
            super::ELF_BASE
        );
        let code = &executable[super::ELF_LOAD_OFFSET..];
        assert!(
            code.windows(4)
                .any(|bytes| bytes == [0x48, 0x83, 0xEC, 0x10])
        );
        assert!(
            code.windows(4)
                .any(|bytes| bytes == [0x48, 0x89, 0x65, 0xF8])
        );
        assert!(
            code.windows(10)
                .any(|bytes| bytes == [0x48, 0xB8, 42, 0, 0, 0, 0, 0, 0, 0])
        );
        assert!(code.windows(2).any(|bytes| bytes == [0xCD, 0x80]));
    }

    #[test]
    fn assembles_local_integer_initialization_and_load() {
        let object = assemble(
            b".globl main\n.text\n.type main, @function\nmain:\npush %rbp\nmov %rsp, %rbp\nsub $16, %rsp\nmov %rsp, -16(%rbp)\nmov $4, %rcx\nlea -4(%rbp), %rdi\nmov $0, %al\nrep stosb\nlea -4(%rbp), %rax\npush %rax\nmov $5, %rax\npop %rdi\nmov %eax, (%rdi)\nlea -4(%rbp), %rax\nmovsxd (%rax), %rax\njmp .L.return.main\nmov $0, %rax\n.L.return.main:\nmov %rbp, %rsp\npop %rbp\nret\n",
        )
        .unwrap();
        let code = &object[24..];
        assert!(code.windows(2).any(|bytes| bytes == [0xF3, 0xAA]));
        assert!(code.windows(2).any(|bytes| bytes == [0x89, 0x07]));
        assert!(code.windows(3).any(|bytes| bytes == [0x48, 0x63, 0x00]));
        assert!(
            code.windows(4)
                .any(|bytes| bytes == [0x48, 0x8D, 0x45, 0xFC])
        );
    }

    #[test]
    fn assembles_integer_arithmetic_and_conditions() {
        let object = assemble(
            b".global _start\n_start:\nadd %edi, %eax\ncmp %edi, %eax\nsetne %al\nmovzb %al, %rax\ncmp $0, %eax\nret\n",
        )
        .unwrap();
        assert_eq!(
            &object[24..],
            &[
                0x01, 0xF8, 0x39, 0xF8, 0x0F, 0x95, 0xC0, 0x48, 0x0F, 0xB6, 0xC0, 0x83, 0xF8, 0x00,
                0xC3,
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
    fn compiles_stdio_literals_and_ignores_installed_standard_headers() {
        let elf = compile_with_includes(
            b"#include <stdio.h>\n#include <stdlib.h>\nint main(void) { puts(\"hello\"); printf(\"world\\n\"); return 0; }",
            "/main.c",
            |_| Err(FsError::NotFound),
        )
        .unwrap();
        assert!(elf.windows(6).any(|bytes| bytes == b"hello\n"));
        assert!(elf.windows(6).any(|bytes| bytes == b"world\n"));
        assert_eq!(
            compile(b"#include <stdio.h>\nint main(void) { printf(\"%d\", 1); return 0; }"),
            Err(ToolchainError::UnsupportedC)
        );
    }

    #[test]
    fn compiles_signed_comparisons_and_short_circuit_logic() {
        let elf = compile(
            b"int main(void) { if (1 < 2 && 2 <= 3 || 4 > 3 && 4 >= 4 && 5 == 5 && 6 != 7 && !0) { puts(\"yes\"); } return 0; }",
        )
        .unwrap();
        let code = &elf[super::ELF_LOAD_OFFSET..];
        for opcode in [
            [0x0F, 0x84],
            [0x0F, 0x85],
            [0x0F, 0x8C],
            [0x0F, 0x8D],
            [0x0F, 0x8E],
            [0x0F, 0x8F],
        ] {
            assert!(code.windows(2).any(|bytes| bytes == opcode));
        }
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
    fn compiles_for_switch_break_and_continue() {
        let elf = compile(
            b"int main(void) { for (int count = 3; count; count = count - 1) { if (count - 2) { continue; } switch (count) { case 1: oct_write(1, \"one\", 3); break; case 2: break; default: oct_write(1, \"other\", 5); } } return 0; }",
        )
        .unwrap();
        let text = &elf[super::ELF_LOAD_OFFSET..];
        assert!(text.windows(3).any(|bytes| bytes == [0x48, 0x39, 0xF8]));
        assert!(text.windows(2).any(|bytes| bytes == [0x0F, 0x84]));
        assert!(text.contains(&0xE9));
        assert_eq!(
            compile(b"int main(void) { break; return 0; }"),
            Err(ToolchainError::InvalidCSource)
        );
        assert_eq!(
            compile(b"int main(void) { switch (1) { case 1: continue; } return 0; }"),
            Err(ToolchainError::InvalidCSource)
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
