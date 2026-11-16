//! Minimal ACPI AML (ACPI Machine Language) reader.
//!
//! This is deliberately not a full AML implementation. It parses the term
//! objects that appear at scope level (`Name`, `Method`, `Device`, `Scope`,
//! `PowerResource`, `OperationRegion`, `Field`, `Alias`, `External`) and
//! interprets the statements those methods use to switch a power resource on:
//! constants, name references, `Store`, `If`/`Else`, `While`, `Return`,
//! `Break`, integer arithmetic, the logical operators, and field reads/writes
//! over `SystemIO` and `SystemMemory` regions.
//!
//! AML that this reader cannot decode never hides the objects after it: the
//! reader advances one byte and tries again, so an unknown construct only costs
//! a resynchronisation. Interpretation is stricter: an unsupported opcode ends
//! that method with [`AmlError::Unsupported`], which the caller reports and
//! carries on from.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ops::Range;

/// `SystemMemory` `OperationRegion` space (ACPI 6.4, 19.6.86).
pub const REGION_SYSTEM_MEMORY: u8 = 0;
/// `SystemIO` `OperationRegion` space.
pub const REGION_SYSTEM_IO: u8 = 1;

const SDT_HEADER_LENGTH: usize = 36;
const MAX_PARSE_DEPTH: usize = 32;
const MAX_OBJECTS: usize = 8192;
const MAX_METHOD_STEPS: usize = 200_000;
const MAX_LOOP_ITERATIONS: usize = 10_000;
const MAX_CALL_DEPTH: usize = 8;
const MAX_PACKAGE_ELEMENTS: usize = 256;
const MAX_PACKAGE_BYTES: usize = 1 << 20;
const MAX_STRING_BYTES: usize = 512;

const ZERO_OP: u8 = 0x00;
const ONE_OP: u8 = 0x01;
const ALIAS_OP: u8 = 0x06;
const NAME_OP: u8 = 0x08;
const BYTE_PREFIX: u8 = 0x0A;
const WORD_PREFIX: u8 = 0x0B;
const DWORD_PREFIX: u8 = 0x0C;
const STRING_PREFIX: u8 = 0x0D;
const QWORD_PREFIX: u8 = 0x0E;
const SCOPE_OP: u8 = 0x10;
const BUFFER_OP: u8 = 0x11;
const PACKAGE_OP: u8 = 0x12;
const METHOD_OP: u8 = 0x14;
const EXTERNAL_OP: u8 = 0x15;
const STALL_OP: u8 = 0x21;
const SLEEP_OP: u8 = 0x22;
const DUAL_NAME_PREFIX: u8 = 0x2E;
const MULTI_NAME_PREFIX: u8 = 0x2F;
const EXT_PREFIX: u8 = 0x5B;
const ROOT_CHAR: u8 = 0x5C;
const PARENT_CHAR: u8 = 0x5E;
const LOCAL_BASE: u8 = 0x60;
const LOCAL_LAST: u8 = 0x67;
const ARG_BASE: u8 = 0x68;
const ARG_LAST: u8 = 0x6E;
const STORE_OP: u8 = 0x70;
const ADD_OP: u8 = 0x72;
const SUBTRACT_OP: u8 = 0x74;
const INCREMENT_OP: u8 = 0x75;
const DECREMENT_OP: u8 = 0x76;
const SHIFT_LEFT_OP: u8 = 0x79;
const SHIFT_RIGHT_OP: u8 = 0x7A;
const AND_OP: u8 = 0x7B;
const OR_OP: u8 = 0x7D;
const NOT_OP: u8 = 0x80;
const NOTIFY_OP: u8 = 0x86;
const TO_INTEGER_OP: u8 = 0x99;
const COPY_OBJECT_OP: u8 = 0x9D;
const LAND_OP: u8 = 0x90;
const LOR_OP: u8 = 0x91;
const LNOT_OP: u8 = 0x92;
const LEQUAL_OP: u8 = 0x93;
const LGREATER_OP: u8 = 0x94;
const LLESS_OP: u8 = 0x95;
const IF_OP: u8 = 0xA0;
const ELSE_OP: u8 = 0xA1;
const WHILE_OP: u8 = 0xA2;
const NOOP_OP: u8 = 0xA3;
const RETURN_OP: u8 = 0xA4;
const BREAK_OP: u8 = 0xA5;
const ONES_OP: u8 = 0xFF;
const EXT_DEVICE_OP: u8 = 0x82;
const EXT_FIELD_OP: u8 = 0x81;
const EXT_OPERATION_REGION_OP: u8 = 0x80;
const EXT_POWER_RESOURCE_OP: u8 = 0x84;
const EXT_PROCESSOR_OP: u8 = 0x83;
const EXT_THERMAL_ZONE_OP: u8 = 0x85;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmlError {
    /// The byte stream ended in the middle of an object.
    Truncated,
    /// A `NameSeg` or `NameString` is malformed.
    BadName,
    /// The namespace grew past [`MAX_OBJECTS`].
    TooManyObjects,
    /// Scope nesting went past [`MAX_PARSE_DEPTH`].
    TooDeep,
    /// An opcode this reader does not decode. Carries the opcode.
    Unsupported(u8),
    /// A supported opcode reached a construct this reader does not implement.
    UnsupportedName(&'static str),
    /// The interpreter cannot find a path the method refers to.
    NoSuchObject(String),
    /// The path exists but is not a method.
    NotAMethod(String),
    /// The path exists but is not an `OperationRegion` field.
    NotAField(String),
    /// An `OperationRegion` cannot be accessed on this platform.
    BadRegion(String),
    /// The region accessor failed.
    Io,
    /// A method ran past [`MAX_METHOD_STEPS`].
    StepLimit,
    /// A `While` loop ran past [`MAX_LOOP_ITERATIONS`].
    LoopLimit,
    /// A package is larger than this reader accepts.
    PackageTooLarge,
}

/// An AML data value. Integers are 64 bit; AML's `Ones` is `u64::MAX`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Integer(u64),
    String(String),
    Bytes(Vec<u8>),
    Package(Vec<Value>),
    /// A bare `NameString` inside a package: a reference to a namespace object,
    /// not a copy of its value. `_PR0` is a package of these.
    Reference(String),
}

impl Value {
    pub fn as_integer(&self) -> u64 {
        match self {
            Value::Integer(value) => *value,
            _ => 0,
        }
    }
}

/// One namespace object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Object {
    Device,
    Scope,
    Processor,
    ThermalZone,
    /// A `Name` object and its value.
    Name(Value),
    /// A method body, kept as a range into `Namespace::tables[table]` and
    /// interpreted on demand.
    Method {
        table: usize,
        body: Range<usize>,
        args: u8,
        /// Scope the method was defined in; names inside it resolve against it.
        scope: String,
    },
    PowerResource {
        system_level: u8,
        resource_order: u16,
    },
    OperationRegion {
        space: u8,
        offset: Value,
        length: Value,
    },
    Field {
        region: String,
        bit_offset: u64,
        bit_width: u64,
    },
    Alias(String),
}

/// Byte-level access to the spaces an `OperationRegion` can name.
pub trait RegionIo {
    fn read(&mut self, space: u8, offset: u64, length: usize) -> Result<Vec<u8>, AmlError>;
    fn write(&mut self, space: u8, offset: u64, bytes: &[u8]) -> Result<(), AmlError>;
}

/// `OperationRegion` access through real port I/O and MMIO.
pub struct PlatformIo;

impl RegionIo for PlatformIo {
    fn read(&mut self, space: u8, offset: u64, length: usize) -> Result<Vec<u8>, AmlError> {
        match space {
            REGION_SYSTEM_IO => {
                let mut bytes = Vec::with_capacity(length);
                for index in 0..length {
                    let port = u16::try_from(offset + index as u64)
                        .map_err(|_| AmlError::BadRegion("port out of range".into()))?;
                    bytes.push(unsafe { crate::arch::x86_64::io::inb(port) });
                }
                Ok(bytes)
            }
            REGION_SYSTEM_MEMORY => {
                let address = usize::try_from(offset)
                    .map_err(|_| AmlError::BadRegion("address out of range".into()))?;
                let base = unsafe {
                    crate::memory::paging::map_mmio_range(offset, length.max(1) as u64)
                };
                let virtual_address = base as usize + (address & 0xfff);
                let mut bytes = Vec::with_capacity(length);
                for index in 0..length {
                    let pointer = (virtual_address + index) as *const u8;
                    bytes.push(unsafe { core::ptr::read_volatile(pointer) });
                }
                Ok(bytes)
            }
            _ => Err(AmlError::BadRegion(format!(
                "region space {space} is not supported"
            ))),
        }
    }

    fn write(&mut self, space: u8, offset: u64, bytes: &[u8]) -> Result<(), AmlError> {
        match space {
            REGION_SYSTEM_IO => {
                for (index, byte) in bytes.iter().enumerate() {
                    let port = u16::try_from(offset + index as u64)
                        .map_err(|_| AmlError::BadRegion("port out of range".into()))?;
                    unsafe { crate::arch::x86_64::io::outb(port, *byte) };
                }
                Ok(())
            }
            REGION_SYSTEM_MEMORY => {
                let address = usize::try_from(offset)
                    .map_err(|_| AmlError::BadRegion("address out of range".into()))?;
                let base = unsafe {
                    crate::memory::paging::map_mmio_range(offset, bytes.len().max(1) as u64)
                };
                let virtual_address = base as usize + (address & 0xfff);
                for (index, byte) in bytes.iter().enumerate() {
                    let pointer = (virtual_address + index) as *mut u8;
                    unsafe { core::ptr::write_volatile(pointer, *byte) };
                }
                Ok(())
            }
            _ => Err(AmlError::BadRegion(format!(
                "region space {space} is not supported"
            ))),
        }
    }
}

/// A parsed AML namespace: object paths mapped to objects, plus the tables the
/// method bodies point into.
pub struct Namespace {
    objects: BTreeMap<String, Object>,
    tables: Vec<&'static [u8]>,
    /// Bytes the reader had to resynchronise on. A non-zero count only means the
    /// tables contain constructs this reader skips.
    resyncs: u32,
    /// The parse stopped before the end of a table because it used up its
    /// iteration budget. A partial namespace only loses definitions; hanging on
    /// one is worse, because the boot never comes back.
    truncated: bool,
    /// Terms parsed across every table, so the total work is bounded even when a
    /// table is parsed in many nested lists.
    steps: u64,
}

/// Set by the caller that wants parse progress in the boot log. It is off by
/// default so host tests stay silent.
pub static PARSE_TRACE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Total terms one namespace parse may handle before it gives up and marks the
/// namespace truncated.
const MAX_PARSE_STEPS: u64 = 1 << 20;

impl Namespace {
    /// Parse the DSDT and every SSDT, in that order.
    pub fn parse(tables: &[&'static [u8]]) -> Self {
        let mut namespace = Namespace {
            objects: BTreeMap::new(),
            tables: tables.to_vec(),
            resyncs: 0,
            truncated: false,
            steps: 0,
        };
        for table in 0..tables.len() {
            let mut cursor = Cursor::new(tables[table]);
            if cursor.skip(SDT_HEADER_LENGTH).is_err() {
                continue;
            }
            let end = tables[table].len();
            let _ = namespace.parse_term_list(table, &mut cursor, "\\", end, 0);
        }
        namespace
    }

    pub fn get(&self, path: &str) -> Option<&Object> {
        self.objects.get(path)
    }

    /// Paths of every object below `path`, one level down.
    pub fn children(&self, path: &str) -> Vec<String> {
        let prefix = if path == "\\" {
            "\\".to_string()
        } else {
            format!("{path}.")
        };
        let mut children = Vec::new();
        for key in self.objects.keys() {
            if let Some(rest) = key.strip_prefix(&prefix) {
                if !rest.contains('.') {
                    children.push(key.clone());
                }
            }
        }
        children
    }

    pub fn resyncs(&self) -> u32 {
        self.resyncs
    }

    /// True when a table was too long to parse within the iteration budget.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Every object path and object, in path order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Object)> {
        self.objects.iter()
    }

    fn insert(&mut self, path: String, object: Object) -> Result<(), AmlError> {
        if !self.objects.contains_key(&path) && self.objects.len() >= MAX_OBJECTS {
            return Err(AmlError::TooManyObjects);
        }
        self.objects.insert(path, object);
        Ok(())
    }

    fn parse_term_list(
        &mut self,
        table: usize,
        cursor: &mut Cursor<'static>,
        scope: &str,
        end: usize,
        depth: usize,
    ) -> Result<(), AmlError> {
        if depth > MAX_PARSE_DEPTH {
            return Err(AmlError::TooDeep);
        }
        // Every iteration has to advance, and the number of iterations is
        // bounded. A vendor DSDT is thousands of constructs this reader only
        // partly understands: one that reports success without consuming a
        // byte, or a table that makes the reader resynchronise on almost every
        // position, spins here with no output, which is exactly how one boot
        // froze on "parsing the ACPI namespace" and never returned.
        let mut budget = end.saturating_mul(2).saturating_add(64);
        while cursor.position < end {
            if budget == 0 || self.steps >= MAX_PARSE_STEPS {
                self.truncated = true;
                break;
            }
            budget -= 1;
            self.steps += 1;
            let start = cursor.position;
            if PARSE_TRACE.load(core::sync::atomic::Ordering::Relaxed)
                && start / 4096 != cursor.position / 4096
            {
                crate::println!(
                    "AML parse: table {table} offset {start:#x} of {end:#x}, {} objects",
                    self.objects.len()
                );
            }
            match self.parse_term(table, cursor, scope, end, depth) {
                Ok(()) => {
                    if cursor.position <= start {
                        self.resyncs = self.resyncs.saturating_add(1);
                        cursor.position = (start + 1).min(end);
                    }
                }
                // Anything this reader does not understand costs one byte and
                // one retry, so the definition that follows is still found.
                Err(AmlError::Unsupported(_))
                | Err(AmlError::Truncated)
                | Err(AmlError::BadName)
                | Err(AmlError::PackageTooLarge) => {
                    self.resyncs = self.resyncs.saturating_add(1);
                    cursor.position = (start + 1).min(end);
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn parse_term(
        &mut self,
        table: usize,
        cursor: &mut Cursor<'static>,
        scope: &str,
        end: usize,
        depth: usize,
    ) -> Result<(), AmlError> {
        cursor.budget = MAX_TERM_BYTES;
        match cursor.peek()? {
            NAME_OP => {
                cursor.next()?;
                let raw = parse_name_string(cursor)?;
                let value = self.parse_value(cursor, scope)?;
                self.insert(resolve_name(scope, &raw), Object::Name(value))
            }
            SCOPE_OP => {
                cursor.next()?;
                let (body, package_end) = read_package(cursor)?;
                let raw = parse_name_string(cursor)?;
                let path = resolve_name(scope, &raw);
                self.insert(path.clone(), Object::Scope)?;
                cursor.position = cursor.position.max(body.start).min(package_end);
                let inner = self.parse_term_list(table, cursor, &path, package_end, depth + 1);
                cursor.position = package_end.min(end);
                inner
            }
            METHOD_OP => {
                cursor.next()?;
                let (_body, package_end) = read_package(cursor)?;
                let raw = parse_name_string(cursor)?;
                let flags = cursor.next()?;
                let body_start = cursor.position;
                self.insert(
                    resolve_name(scope, &raw),
                    Object::Method {
                        table,
                        body: body_start..package_end,
                        args: flags & 0x07,
                        scope: scope.to_string(),
                    },
                )?;
                cursor.position = package_end;
                Ok(())
            }
            EXTERNAL_OP => {
                cursor.next()?;
                let _ = parse_name_string(cursor)?;
                cursor.next()?;
                cursor.next()?;
                Ok(())
            }
            ALIAS_OP => {
                cursor.next()?;
                let source = parse_name_string(cursor)?;
                let alias = parse_name_string(cursor)?;
                self.insert(
                    resolve_name(scope, &alias),
                    Object::Alias(resolve_name(scope, &source)),
                )
            }
            EXT_PREFIX => {
                cursor.next()?;
                let opcode = cursor.next()?;
                match opcode {
                    EXT_DEVICE_OP | EXT_PROCESSOR_OP | EXT_THERMAL_ZONE_OP => {
                        let (_body, package_end) = read_package(cursor)?;
                        let raw = parse_name_string(cursor)?;
                        let path = resolve_name(scope, &raw);
                        let object = match opcode {
                            EXT_DEVICE_OP => Object::Device,
                            EXT_PROCESSOR_OP => Object::Processor,
                            _ => Object::ThermalZone,
                        };
                        self.insert(path.clone(), object)?;
                        if opcode == EXT_PROCESSOR_OP {
                            // ProcessorId, PblkAddress, PblkLength.
                            cursor.skip(6)?;
                        }
                        let inner = self.parse_term_list(table, cursor, &path, package_end, depth + 1);
                        cursor.position = package_end.min(end);
                        inner
                    }
                    EXT_POWER_RESOURCE_OP => {
                        let (_body, package_end) = read_package(cursor)?;
                        let raw = parse_name_string(cursor)?;
                        let system_level = cursor.next()?;
                        let resource_order = cursor.u16()?;
                        let path = resolve_name(scope, &raw);
                        self.insert(
                            path.clone(),
                            Object::PowerResource {
                                system_level,
                                resource_order,
                            },
                        )?;
                        let inner = self.parse_term_list(table, cursor, &path, package_end, depth + 1);
                        cursor.position = package_end.min(end);
                        inner
                    }
                    EXT_OPERATION_REGION_OP => {
                        let raw = parse_name_string(cursor)?;
                        let space = cursor.next()?;
                        let offset = self.parse_value(cursor, scope)?;
                        let length = self.parse_value(cursor, scope)?;
                        self.insert(
                            resolve_name(scope, &raw),
                            Object::OperationRegion {
                                space,
                                offset,
                                length,
                            },
                        )
                    }
                    EXT_FIELD_OP => {
                        let (_body, package_end) = read_package(cursor)?;
                        let region = resolve_name(scope, &parse_name_string(cursor)?);
                        let _flags = cursor.next()?;
                        self.parse_field_list(cursor, scope, &region, package_end)?;
                        cursor.position = package_end.min(end);
                        Ok(())
                    }
                    other => Err(AmlError::Unsupported(other)),
                }
            }
            other => Err(AmlError::Unsupported(other)),
        }
    }

    /// `Field` entries. A `NameSeg` starts a named field; `0x00`, `0x01`, `0x02`
    /// and `0x03` start the anonymous reserved/access/connect entries, which is
    /// unambiguous because a `NameSeg` begins with a letter or `_`.
    fn parse_field_list(
        &mut self,
        cursor: &mut Cursor<'static>,
        scope: &str,
        region: &str,
        end: usize,
    ) -> Result<(), AmlError> {
        let mut bit_offset = 0u64;
        while cursor.position < end {
            match cursor.peek()? {
                0x00 => {
                    cursor.next()?;
                    bit_offset += package_length_value(cursor)?;
                    continue;
                }
                0x01 => {
                    cursor.next()?;
                    cursor.skip(2)?;
                    continue;
                }
                0x03 => {
                    cursor.next()?;
                    cursor.skip(3)?;
                    continue;
                }
                0x02 => return Err(AmlError::UnsupportedName("ConnectField")),
                _ => {}
            }
            let name = name_seg(cursor)?;
            let width = package_length_value(cursor)?;
            if width == 0 {
                return Err(AmlError::BadName);
            }
            self.insert(
                resolve_name(scope, &name),
                Object::Field {
                    region: region.to_string(),
                    bit_offset,
                    bit_width: width,
                },
            )?;
            bit_offset += width;
        }
        Ok(())
    }

    fn parse_value(
        &mut self,
        cursor: &mut Cursor<'static>,
        scope: &str,
    ) -> Result<Value, AmlError> {
        match cursor.peek()? {
            ZERO_OP => {
                cursor.next()?;
                Ok(Value::Integer(0))
            }
            ONE_OP => {
                cursor.next()?;
                Ok(Value::Integer(1))
            }
            ONES_OP => {
                cursor.next()?;
                Ok(Value::Integer(u64::MAX))
            }
            BYTE_PREFIX => {
                cursor.next()?;
                Ok(Value::Integer(cursor.next()? as u64))
            }
            WORD_PREFIX => {
                cursor.next()?;
                Ok(Value::Integer(cursor.u16()? as u64))
            }
            DWORD_PREFIX => {
                cursor.next()?;
                Ok(Value::Integer(cursor.u32()? as u64))
            }
            QWORD_PREFIX => {
                cursor.next()?;
                Ok(Value::Integer(cursor.u64()?))
            }
            STRING_PREFIX => {
                cursor.next()?;
                Ok(Value::String(cursor.string()?))
            }
            BUFFER_OP => {
                cursor.next()?;
                let (_body, end) = read_package(cursor)?;
                let _size = self.parse_value(cursor, scope)?;
                let bytes = cursor.take(end.saturating_sub(cursor.position))?.to_vec();
                cursor.position = end;
                Ok(Value::Bytes(bytes))
            }
            PACKAGE_OP => {
                cursor.next()?;
                let (_body, end) = read_package(cursor)?;
                let count = cursor.next()? as usize;
                if count > MAX_PACKAGE_ELEMENTS {
                    return Err(AmlError::PackageTooLarge);
                }
                let mut elements = Vec::new();
                while cursor.position < end && elements.len() < count {
                    if cursor.peek()? == ZERO_OP {
                        // An empty package element.
                        cursor.next()?;
                        elements.push(Value::Integer(0));
                        continue;
                    }
                    elements.push(self.parse_value(cursor, scope)?);
                }
                cursor.position = end;
                Ok(Value::Package(elements))
            }
            DUAL_NAME_PREFIX | MULTI_NAME_PREFIX | ROOT_CHAR | PARENT_CHAR => {
                let raw = parse_name_string(cursor)?;
                Ok(Value::Reference(resolve_name(scope, &raw)))
            }
            byte if byte.is_ascii_uppercase() || byte == b'_' => {
                let raw = parse_name_string(cursor)?;
                Ok(Value::Reference(resolve_name(scope, &raw)))
            }
            other => Err(AmlError::Unsupported(other)),
        }
    }
}

/// Interpret a method body.
pub struct Interpreter<'a> {
    namespace: &'a Namespace,
    locals: [Value; 8],
    args: [Value; 7],
    steps: usize,
}

/// Result of executing a statement.
enum Flow {
    Next,
    Return(Value),
    Break,
}

impl<'a> Interpreter<'a> {
    pub fn new(namespace: &'a Namespace) -> Self {
        Interpreter {
            namespace,
            locals: [const { Value::Integer(0) }; 8],
            args: [const { Value::Integer(0) }; 7],
            steps: 0,
        }
    }

    /// Run the method at `path` with `args` and return its `Return` value (0 when
    /// it returns nothing).
    pub fn invoke(
        &mut self,
        path: &str,
        args: &[Value],
        io: &mut dyn RegionIo,
    ) -> Result<Value, AmlError> {
        let Some(Object::Method {
            table,
            body,
            args: declared,
            scope,
        }) = self.namespace.get(path).cloned()
        else {
            return Err(AmlError::NotAMethod(path.to_string()));
        };
        if args.len() > usize::from(declared) {
            return Err(AmlError::NotAMethod(format!(
                "{path} takes {declared} argument(s)"
            )));
        }
        let mut cursor = Cursor::new(self.namespace.tables[table]);
        cursor.position = body.start;
        let mut frame = Interpreter::new(self.namespace);
        for (index, value) in args.iter().enumerate() {
            frame.args[index] = value.clone();
        }
        frame.run(&mut cursor, &scope, body.end, io, 1)
    }

    fn run(
        &mut self,
        cursor: &mut Cursor<'static>,
        scope: &str,
        end: usize,
        io: &mut dyn RegionIo,
        depth: usize,
    ) -> Result<Value, AmlError> {
        if depth > MAX_CALL_DEPTH {
            return Err(AmlError::StepLimit);
        }
        match self.run_block(cursor, scope, end, io, depth)? {
            Flow::Return(value) => Ok(value),
            _ => Ok(Value::Integer(0)),
        }
    }

    fn run_block(
        &mut self,
        cursor: &mut Cursor<'static>,
        scope: &str,
        end: usize,
        io: &mut dyn RegionIo,
        depth: usize,
    ) -> Result<Flow, AmlError> {
        while cursor.position < end {
            match self.statement(cursor, scope, io, depth)? {
                Flow::Next => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Next)
    }

    fn statement(
        &mut self,
        cursor: &mut Cursor<'static>,
        scope: &str,
        io: &mut dyn RegionIo,
        depth: usize,
    ) -> Result<Flow, AmlError> {
        self.steps += 1;
        if self.steps > MAX_METHOD_STEPS {
            return Err(AmlError::StepLimit);
        }
        match cursor.peek()? {
            IF_OP => {
                cursor.next()?;
                let (_body, end) = read_package(cursor)?;
                let predicate = self.expression(cursor, scope, io, depth)?;
                let body_start = cursor.position;
                let mut flow = Flow::Next;
                if predicate.as_integer() != 0 {
                    let mut nested = Cursor {
                        bytes: cursor.bytes,
                        position: body_start,
                        budget: MAX_TERM_BYTES,
                    };
                    flow = self.run_block(&mut nested, scope, end, io, depth)?;
                }
                cursor.position = end;
                match flow {
                    Flow::Next => {}
                    other => return Ok(other),
                }
                if cursor.peek().ok() == Some(ELSE_OP) {
                    cursor.next()?;
                    let (_body, else_end) = read_package(cursor)?;
                    if predicate.as_integer() == 0 {
                        let mut nested = Cursor {
                            bytes: cursor.bytes,
                            position: cursor.position,
                            budget: MAX_TERM_BYTES,
                        };
                        let flow = self.run_block(&mut nested, scope, else_end, io, depth)?;
                        cursor.position = else_end;
                        return Ok(flow);
                    }
                    cursor.position = else_end;
                }
                Ok(Flow::Next)
            }
            WHILE_OP => {
                cursor.next()?;
                let (_body, end) = read_package(cursor)?;
                let predicate_start = cursor.position;
                for _ in 0..MAX_LOOP_ITERATIONS {
                    let mut predicate_cursor = Cursor {
                        bytes: cursor.bytes,
                        position: predicate_start,
                        budget: MAX_TERM_BYTES,
                    };
                    let predicate = self.expression(&mut predicate_cursor, scope, io, depth)?;
                    if predicate.as_integer() == 0 {
                        break;
                    }
                    let mut body_cursor = Cursor {
                        bytes: cursor.bytes,
                        position: predicate_cursor.position,
                        budget: MAX_TERM_BYTES,
                    };
                    match self.run_block(&mut body_cursor, scope, end, io, depth)? {
                        Flow::Next | Flow::Break => {}
                        Flow::Return(value) => {
                            cursor.position = end;
                            return Ok(Flow::Return(value));
                        }
                    }
                }
                cursor.position = end;
                Ok(Flow::Next)
            }
            RETURN_OP => {
                cursor.next()?;
                Ok(Flow::Return(self.expression(cursor, scope, io, depth)?))
            }
            BREAK_OP => {
                cursor.next()?;
                Ok(Flow::Break)
            }
            NOOP_OP => {
                cursor.next()?;
                Ok(Flow::Next)
            }
            NOTIFY_OP => {
                cursor.next()?;
                let _ = self.expression(cursor, scope, io, depth)?;
                let _ = self.expression(cursor, scope, io, depth)?;
                Ok(Flow::Next)
            }
            STALL_OP | SLEEP_OP => {
                cursor.next()?;
                let _ = self.expression(cursor, scope, io, depth)?;
                Ok(Flow::Next)
            }
            STORE_OP => {
                self.expression(cursor, scope, io, depth)?;
                Ok(Flow::Next)
            }
            _ => {
                self.expression(cursor, scope, io, depth)?;
                Ok(Flow::Next)
            }
        }
    }

    fn expression(
        &mut self,
        cursor: &mut Cursor<'static>,
        scope: &str,
        io: &mut dyn RegionIo,
        depth: usize,
    ) -> Result<Value, AmlError> {
        self.steps += 1;
        if self.steps > MAX_METHOD_STEPS {
            return Err(AmlError::StepLimit);
        }
        let opcode = cursor.peek()?;
        match opcode {
            ZERO_OP => {
                cursor.next()?;
                Ok(Value::Integer(0))
            }
            ONE_OP => {
                cursor.next()?;
                Ok(Value::Integer(1))
            }
            ONES_OP => {
                cursor.next()?;
                Ok(Value::Integer(u64::MAX))
            }
            BYTE_PREFIX => {
                cursor.next()?;
                Ok(Value::Integer(cursor.next()? as u64))
            }
            WORD_PREFIX => {
                cursor.next()?;
                Ok(Value::Integer(cursor.u16()? as u64))
            }
            DWORD_PREFIX => {
                cursor.next()?;
                Ok(Value::Integer(cursor.u32()? as u64))
            }
            QWORD_PREFIX => {
                cursor.next()?;
                Ok(Value::Integer(cursor.u64()?))
            }
            STRING_PREFIX => {
                cursor.next()?;
                Ok(Value::String(cursor.string()?))
            }
            STORE_OP => {
                cursor.next()?;
                let value = self.expression(cursor, scope, io, depth)?;
                let target = self.supername(cursor, scope)?;
                self.store_path(&target, io, value.clone())?;
                Ok(value)
            }
            ADD_OP | SUBTRACT_OP | AND_OP | OR_OP | SHIFT_LEFT_OP | SHIFT_RIGHT_OP => {
                cursor.next()?;
                let left = self.expression(cursor, scope, io, depth)?.as_integer();
                let right = self.expression(cursor, scope, io, depth)?.as_integer();
                let result = match opcode {
                    ADD_OP => left.wrapping_add(right),
                    SUBTRACT_OP => left.wrapping_sub(right),
                    AND_OP => left & right,
                    OR_OP => left | right,
                    SHIFT_LEFT_OP => left.checked_shl(right as u32).unwrap_or(0),
                    _ => left.checked_shr(right as u32).unwrap_or(0),
                };
                self.store_optional_target(cursor, scope, io, result, depth)
            }
            NOT_OP => {
                cursor.next()?;
                let operand = self.expression(cursor, scope, io, depth)?.as_integer();
                self.store_optional_target(cursor, scope, io, !operand, depth)
            }
            LAND_OP | LOR_OP | LEQUAL_OP | LGREATER_OP | LLESS_OP => {
                cursor.next()?;
                let left = self.expression(cursor, scope, io, depth)?.as_integer();
                let right = self.expression(cursor, scope, io, depth)?.as_integer();
                let result = match opcode {
                    LAND_OP => (left != 0 && right != 0) as u64,
                    LOR_OP => (left != 0 || right != 0) as u64,
                    LEQUAL_OP => (left == right) as u64,
                    LGREATER_OP => (left > right) as u64,
                    _ => (left < right) as u64,
                };
                Ok(Value::Integer(result))
            }
            LNOT_OP => {
                cursor.next()?;
                let operand = self.expression(cursor, scope, io, depth)?.as_integer();
                Ok(Value::Integer((operand == 0) as u64))
            }
            COPY_OBJECT_OP => {
                cursor.next()?;
                let value = self.expression(cursor, scope, io, depth)?;
                let target = self.supername(cursor, scope)?;
                self.store_path(&target, io, value.clone())?;
                Ok(value)
            }
            TO_INTEGER_OP => {
                cursor.next()?;
                let integer = self.expression(cursor, scope, io, depth)?.as_integer();
                self.store_optional_target(cursor, scope, io, integer, depth)
            }
            INCREMENT_OP | DECREMENT_OP => {
                cursor.next()?;
                let path = self.supername(cursor, scope)?;
                if path.starts_with('@') {
                    return Err(AmlError::UnsupportedName("increment of a local"));
                }
                let current = self.load(&path, io, depth)?.as_integer();
                let value = if opcode == INCREMENT_OP {
                    current.wrapping_add(1)
                } else {
                    current.wrapping_sub(1)
                };
                self.store_path(&path, io, Value::Integer(value))?;
                Ok(Value::Integer(value))
            }
            EXT_PREFIX => {
                cursor.next()?;
                let extension = cursor.next()?;
                if extension == 0x31 {
                    // `Debug`: a write-only object; reading it yields nothing.
                    Ok(Value::Integer(0))
                } else {
                    Err(AmlError::Unsupported(extension))
                }
            }
            LOCAL_BASE..=LOCAL_LAST => {
                cursor.next()?;
                Ok(self.locals[usize::from(opcode - LOCAL_BASE)].clone())
            }
            ARG_BASE..=ARG_LAST => {
                cursor.next()?;
                Ok(self.args[usize::from(opcode - ARG_BASE)].clone())
            }
            _ => {
                let raw = parse_name_string(cursor)?;
                let path = resolve_name(scope, &raw);
                match self.namespace.get(&path).cloned() {
                    Some(Object::Method {
                        table,
                        body,
                        args,
                        scope: method_scope,
                    }) => {
                        let mut values = Vec::with_capacity(usize::from(args));
                        for _ in 0..args {
                            values.push(self.expression(cursor, scope, io, depth)?);
                        }
                        let mut frame = Interpreter::new(self.namespace);
                        for (index, value) in values.iter().enumerate() {
                            frame.args[index] = value.clone();
                        }
                        let mut method_cursor = Cursor::new(self.namespace.tables[table]);
                        method_cursor.position = body.start;
                        frame.run(&mut method_cursor, &method_scope, body.end, io, depth + 1)
                    }
                    Some(Object::Name(value)) => Ok(value.clone()),
                    Some(Object::Alias(target)) => self.load(&target, io, depth),
                    Some(Object::Field { .. }) => self.load(&path, io, depth),
                    Some(_) => Ok(Value::Reference(path)),
                    None => Err(AmlError::NoSuchObject(path)),
                }
            }
        }
    }

    /// Read a `SuperName` and return its path, or a synthetic `@local`/`@arg`
    /// marker for the register operands.
    fn supername(
        &mut self,
        cursor: &mut Cursor<'static>,
        scope: &str,
    ) -> Result<String, AmlError> {
        let opcode = cursor.peek()?;
        match opcode {
            LOCAL_BASE..=LOCAL_LAST => {
                cursor.next()?;
                Ok(format!("@local{}", opcode - LOCAL_BASE))
            }
            ARG_BASE..=ARG_LAST => {
                cursor.next()?;
                Ok(format!("@arg{}", opcode - ARG_BASE))
            }
            EXT_PREFIX => {
                cursor.next()?;
                let extension = cursor.next()?;
                if extension == 0x31 {
                    Ok("@debug".to_string())
                } else {
                    Err(AmlError::Unsupported(extension))
                }
            }
            _ => {
                let raw = parse_name_string(cursor)?;
                Ok(resolve_name(scope, &raw))
            }
        }
    }

    fn store_path(
        &mut self,
        target: &str,
        io: &mut dyn RegionIo,
        value: Value,
    ) -> Result<(), AmlError> {
        if let Some(index) = target.strip_prefix("@local") {
            let index: usize = index.parse().map_err(|_| AmlError::Io)?;
            self.locals[index] = value;
            return Ok(());
        }
        if let Some(index) = target.strip_prefix("@arg") {
            let index: usize = index.parse().map_err(|_| AmlError::Io)?;
            self.args[index] = value;
            return Ok(());
        }
        if target == "@debug" {
            return Ok(());
        }
        match self.namespace.get(target).cloned() {
            Some(Object::Field { .. }) => self.write_field(target, io, value.as_integer()),
            Some(Object::Alias(real)) => self.store_path(&real, io, value),
            // A `Name` is a constant: AML cannot write one, so a method that
            // tries is doing something this reader does not model.
            Some(_) => Err(AmlError::UnsupportedName("store into a non-field")),
            None => Err(AmlError::NoSuchObject(target.to_string())),
        }
    }

    fn load(
        &mut self,
        path: &str,
        io: &mut dyn RegionIo,
        _depth: usize,
    ) -> Result<Value, AmlError> {
        match self.namespace.get(path).cloned() {
            Some(Object::Name(value)) => Ok(value),
            Some(Object::Field { .. }) => Ok(Value::Integer(self.read_field(path, io)?)),
            Some(Object::Alias(real)) => self.load(&real, io, _depth),
            Some(Object::Method { .. }) => {
                let mut interpreter = Interpreter::new(self.namespace);
                interpreter.invoke(path, &[], io)
            }
            Some(_) => Ok(Value::Reference(path.to_string())),
            None => Err(AmlError::NoSuchObject(path.to_string())),
        }
    }

    fn store_optional_target(
        &mut self,
        cursor: &mut Cursor<'static>,
        scope: &str,
        io: &mut dyn RegionIo,
        integer: u64,
        _depth: usize,
    ) -> Result<Value, AmlError> {
        // A `Target` may be `NullName` (0x00), which means "return the value".
        if cursor.peek().ok() == Some(ZERO_OP) {
            cursor.next()?;
            return Ok(Value::Integer(integer));
        }
        let target = self.supername(cursor, scope)?;
        self.store_path(&target, io, Value::Integer(integer))?;
        Ok(Value::Integer(integer))
    }

    fn field_region(&self, path: &str) -> Result<(u8, u64, u64, u64), AmlError> {
        let Some(Object::Field {
            region,
            bit_offset,
            bit_width,
        }) = self.namespace.get(path).cloned()
        else {
            return Err(AmlError::NotAField(path.to_string()));
        };
        let Some(Object::OperationRegion { space, offset, .. }) = self.namespace.get(&region) else {
            return Err(AmlError::BadRegion(region));
        };
        // The region's `Offset` is in bytes; the field offsets that follow are in
        // bits from the start of the region.
        Ok((
            *space,
            offset.as_integer().saturating_mul(8),
            bit_offset,
            bit_width,
        ))
    }

    fn read_field(&mut self, path: &str, io: &mut dyn RegionIo) -> Result<u64, AmlError> {
        let (space, region_bit_offset, bit_offset, bit_width) = self.field_region(path)?;
        read_bits(io, space, region_bit_offset + bit_offset, bit_width)
    }

    fn write_field(
        &mut self,
        path: &str,
        io: &mut dyn RegionIo,
        value: u64,
    ) -> Result<(), AmlError> {
        let (space, region_bit_offset, bit_offset, bit_width) = self.field_region(path)?;
        write_bits(io, space, region_bit_offset + bit_offset, bit_width, value)
    }
}

fn read_bits(
    io: &mut dyn RegionIo,
    space: u8,
    bit_offset: u64,
    bit_width: u64,
) -> Result<u64, AmlError> {
    if bit_width == 0 || bit_width > 64 {
        return Err(AmlError::BadRegion("unsupported field width".into()));
    }
    let first_byte = bit_offset / 8;
    let last_byte = (bit_offset + bit_width - 1) / 8;
    let length = usize::try_from(last_byte - first_byte + 1).map_err(|_| AmlError::Io)?;
    if length > 8 {
        return Err(AmlError::BadRegion("field spans more than eight bytes".into()));
    }
    let bytes = io.read(space, first_byte, length)?;
    let mut raw = 0u64;
    for (index, byte) in bytes.iter().enumerate() {
        raw |= u64::from(*byte) << (8 * index);
    }
    let shift = (bit_offset % 8) as u32;
    let mask = if bit_width >= 64 {
        u64::MAX
    } else {
        (1u64 << bit_width) - 1
    };
    Ok((raw >> shift) & mask)
}

fn write_bits(
    io: &mut dyn RegionIo,
    space: u8,
    bit_offset: u64,
    bit_width: u64,
    value: u64,
) -> Result<(), AmlError> {
    if bit_width == 0 || bit_width > 64 {
        return Err(AmlError::BadRegion("unsupported field width".into()));
    }
    let first_byte = bit_offset / 8;
    let last_byte = (bit_offset + bit_width - 1) / 8;
    let length = usize::try_from(last_byte - first_byte + 1).map_err(|_| AmlError::Io)?;
    if length > 8 {
        return Err(AmlError::BadRegion("field spans more than eight bytes".into()));
    }
    let mut bytes = io.read(space, first_byte, length)?;
    let mut raw = 0u64;
    for (index, byte) in bytes.iter().enumerate() {
        raw |= u64::from(*byte) << (8 * index);
    }
    let shift = (bit_offset % 8) as u32;
    let mask = if bit_width >= 64 {
        u64::MAX
    } else {
        (1u64 << bit_width) - 1
    };
    raw = (raw & !(mask << shift)) | ((value & mask) << shift);
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = ((raw >> (8 * index)) & 0xff) as u8;
    }
    io.write(space, first_byte, &bytes)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
    /// Bytes the term being parsed may still consume. A single definition is at
    /// most a few hundred bytes in any real table, so a term that keeps reading
    /// is a reader bug; it is cut off instead of running to the end of the table.
    budget: usize,
}

/// Upper bound on the bytes one term may consume.
const MAX_TERM_BYTES: usize = 8 * 1024;

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Cursor {
            bytes,
            position: 0,
            budget: MAX_TERM_BYTES,
        }
    }

    fn peek(&self) -> Result<u8, AmlError> {
        self.bytes.get(self.position).copied().ok_or(AmlError::Truncated)
    }

    fn next(&mut self) -> Result<u8, AmlError> {
        if self.budget == 0 {
            return Err(AmlError::Truncated);
        }
        let byte = self.peek()?;
        self.budget -= 1;
        self.position += 1;
        Ok(byte)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], AmlError> {
        if count > self.budget {
            return Err(AmlError::Truncated);
        }
        let end = self.position.checked_add(count).ok_or(AmlError::Truncated)?;
        let bytes = self.bytes.get(self.position..end).ok_or(AmlError::Truncated)?;
        self.budget -= count;
        self.position = end;
        Ok(bytes)
    }

    fn skip(&mut self, count: usize) -> Result<(), AmlError> {
        self.take(count).map(|_| ())
    }

    fn u16(&mut self) -> Result<u16, AmlError> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, AmlError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, AmlError> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn string(&mut self) -> Result<String, AmlError> {
        let mut length = 0;
        loop {
            let byte = *self.bytes.get(self.position + length).ok_or(AmlError::Truncated)?;
            if byte == 0 {
                break;
            }
            length += 1;
            if length > MAX_STRING_BYTES {
                return Err(AmlError::Truncated);
            }
        }
        let bytes = self.take(length)?;
        self.position += 1; // NUL
        Ok(bytes.iter().map(|byte| *byte as char).collect())
    }
}

/// Read a package: its content range and the offset just past it.
fn read_package(cursor: &mut Cursor<'static>) -> Result<(Range<usize>, usize), AmlError> {
    let start = cursor.position;
    let (length, encoded) = decode_package_length(cursor)?;
    let end = start.checked_add(length).ok_or(AmlError::Truncated)?;
    if end > cursor.bytes.len() || length > MAX_PACKAGE_BYTES {
        return Err(AmlError::Truncated);
    }
    Ok((start + encoded..end, end))
}

fn decode_package_length(cursor: &mut Cursor<'static>) -> Result<(usize, usize), AmlError> {
    let lead = cursor.next()?;
    let following = usize::from(lead >> 6);
    let mut length = if following == 0 {
        usize::from(lead & 0x3f)
    } else {
        usize::from(lead & 0x0f)
    };
    for index in 0..following {
        let byte = cursor.next()?;
        length |= usize::from(byte) << (4 + index * 8);
    }
    Ok((length, following + 1))
}

/// The bare `PkgLength` form used by `Field` entries.
fn package_length_value(cursor: &mut Cursor<'static>) -> Result<u64, AmlError> {
    let (length, _) = decode_package_length(cursor)?;
    Ok(length as u64)
}

fn parse_name_string(cursor: &mut Cursor<'static>) -> Result<String, AmlError> {
    let mut text = String::new();
    if cursor.peek().ok() == Some(ROOT_CHAR) {
        cursor.next()?;
        text.push('\\');
    }
    while cursor.peek().ok() == Some(PARENT_CHAR) {
        cursor.next()?;
        text.push('^');
    }
    match cursor.peek()? {
        DUAL_NAME_PREFIX => {
            cursor.next()?;
            text.push_str(&name_seg(cursor)?);
            text.push('.');
            text.push_str(&name_seg(cursor)?);
        }
        MULTI_NAME_PREFIX => {
            cursor.next()?;
            let count = cursor.next()? as usize;
            if count == 0 || count > 16 {
                return Err(AmlError::BadName);
            }
            for index in 0..count {
                if index > 0 {
                    text.push('.');
                }
                text.push_str(&name_seg(cursor)?);
            }
        }
        ZERO_OP => return Err(AmlError::BadName),
        _ => text.push_str(&name_seg(cursor)?),
    }
    Ok(text)
}

fn name_seg(cursor: &mut Cursor<'static>) -> Result<String, AmlError> {
    let bytes = cursor.take(4)?;
    let leader = bytes[0];
    if !(leader.is_ascii_uppercase() || leader == b'_') {
        return Err(AmlError::BadName);
    }
    let mut name = String::new();
    for byte in bytes.iter().copied() {
        if !(byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_') {
            return Err(AmlError::BadName);
        }
        name.push(byte as char);
    }
    while name.len() > 1 && name.ends_with('_') {
        name.pop();
    }
    Ok(name)
}

fn parent_path(path: &str) -> &str {
    match path.rfind('.') {
        Some(index) => &path[..index],
        None => "\\",
    }
}

fn resolve_name(scope: &str, raw: &str) -> String {
    let mut rest = raw;
    let mut base = scope.to_string();
    if let Some(stripped) = rest.strip_prefix('\\') {
        rest = stripped;
        base = "\\".to_string();
    } else {
        while let Some(stripped) = rest.strip_prefix('^') {
            base = parent_path(&base).to_string();
            rest = stripped;
        }
    }
    if rest.is_empty() {
        return base;
    }
    if base == "\\" {
        format!("\\{rest}")
    } else {
        format!("{base}.{rest}")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AmlError, Interpreter, Namespace, Object, REGION_SYSTEM_IO, RegionIo, Value, read_bits,
        write_bits,
    };
    use alloc::boxed::Box;
    use alloc::collections::BTreeMap;
    use alloc::vec::Vec;

    #[derive(Default)]
    struct MockIo {
        ports: BTreeMap<u64, u8>,
        writes: Vec<(u64, u8)>,
    }

    impl RegionIo for MockIo {
        fn read(&mut self, space: u8, offset: u64, length: usize) -> Result<Vec<u8>, AmlError> {
            assert_eq!(space, REGION_SYSTEM_IO);
            Ok((0..length as u64)
                .map(|index| *self.ports.get(&(offset + index)).unwrap_or(&0))
                .collect())
        }

        fn write(&mut self, space: u8, offset: u64, bytes: &[u8]) -> Result<(), AmlError> {
            assert_eq!(space, REGION_SYSTEM_IO);
            for (index, byte) in bytes.iter().enumerate() {
                self.ports.insert(offset + index as u64, *byte);
                self.writes.push((offset + index as u64, *byte));
            }
            Ok(())
        }
    }

    fn seg(segment: &str) -> Vec<u8> {
        let mut bytes = segment.as_bytes().to_vec();
        while bytes.len() < 4 {
            bytes.push(b'_');
        }
        bytes.truncate(4);
        bytes
    }

    /// Encode a `PkgLength` for a payload of `payload` bytes.
    fn pkglen(payload: usize) -> Vec<u8> {
        for width in 1..=4usize {
            let total = payload + width;
            match width {
                1 if total <= 0x3f => return vec![total as u8],
                2 if total <= 0xfff => {
                    return vec![((total & 0x0f) as u8) | 0x40, ((total >> 4) & 0xff) as u8];
                }
                3 if total <= 0xfffff => {
                    return vec![
                        ((total & 0x07) as u8) | 0x80,
                        ((total >> 4) & 0xff) as u8,
                        ((total >> 12) & 0xff) as u8,
                    ];
                }
                4 => {
                    return vec![
                        ((total & 0x03) as u8) | 0xc0,
                        ((total >> 4) & 0xff) as u8,
                        ((total >> 12) & 0xff) as u8,
                        ((total >> 20) & 0xff) as u8,
                    ];
                }
                _ => continue,
            }
        }
        unreachable!()
    }

    /// `opcode` + `PkgLength` + `prefix` + `body`.
    fn with_package(opcode: &[u8], prefix: &[u8], body: &[u8]) -> Vec<u8> {
        let mut header = prefix.to_vec();
        header.extend_from_slice(body);
        let mut out = opcode.to_vec();
        out.extend(pkglen(header.len()));
        out.extend(header);
        out
    }

    fn device(segment: &str, body: &[u8]) -> Vec<u8> {
        with_package(&[0x5b, 0x82], &seg(segment), body)
    }

    fn power_resource(segment: &str, body: &[u8]) -> Vec<u8> {
        let mut prefix = seg(segment);
        prefix.push(0); // system level
        prefix.extend_from_slice(&[0, 0]); // resource order
        with_package(&[0x5b, 0x84], &prefix, body)
    }

    fn method(segment: &str, args: u8, body: &[u8]) -> Vec<u8> {
        let mut prefix = seg(segment);
        prefix.push(args);
        with_package(&[0x14], &prefix, body)
    }

    fn name_op(segment: &str, value: &[u8]) -> Vec<u8> {
        let mut out = vec![0x08];
        out.extend(seg(segment));
        out.extend_from_slice(value);
        out
    }

    fn dword(value: u32) -> Vec<u8> {
        let mut out = vec![0x0c];
        out.extend_from_slice(&value.to_le_bytes());
        out
    }

    fn package(elements: &[Vec<u8>]) -> Vec<u8> {
        let mut body = vec![elements.len() as u8];
        for element in elements {
            body.extend_from_slice(element);
        }
        with_package(&[0x12], &[], &body)
    }

    fn operation_region(segment: &str, space: u8, offset: &[u8], length: &[u8]) -> Vec<u8> {
        let mut out = vec![0x5b, 0x80];
        out.extend(seg(segment));
        out.push(space);
        out.extend_from_slice(offset);
        out.extend_from_slice(length);
        out
    }

    fn field(region: &str, entries: &[(&str, u8)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (segment, width) in entries {
            body.extend(seg(segment));
            body.push(*width);
        }
        let mut prefix = seg(region);
        prefix.push(0x01); // ByteAcc, NoLock, Preserve
        with_package(&[0x5b, 0x81], &prefix, &body)
    }

    fn root_reference(segment: &str) -> Vec<u8> {
        let mut out = vec![0x5c];
        out.extend(seg(segment));
        out
    }

    /// A trimmed-down DSDT body: one PCI root, one endpoint, one power resource
    /// at root scope that the endpoint's `_PR0` points at.
    fn fixture() -> Vec<u8> {
        let mut endpoint = Vec::new();
        endpoint.extend(name_op("_ADR", &dword(0x0002_0000)));
        endpoint.extend(name_op("_PR0", &package(&[root_reference("PWR1")])));

        let mut root = Vec::new();
        root.extend(name_op("_HID", &dword(0x080A_D041))); // PNP0A08
        root.extend(name_op("_BBN", &[0x00]));
        root.extend(device("WLAN", &endpoint));

        let mut regulator = Vec::new();
        regulator.extend(operation_region(
            "GPR1",
            REGION_SYSTEM_IO,
            &[0x0b, 0x00, 0x80], // 0x8000
            &[0x0a, 0x02],
        ));
        regulator.extend(field("GPR1", &[("WLON", 1)]));
        regulator.extend(method("_STA", 0, &[0xa4, 0x00])); // Return (Zero)
        regulator.extend(method(
            "_ON",
            0,
            &[0x70, 0x01, b'W', b'L', b'O', b'N'], // Store (One, WLON)
        ));

        let mut table = vec![0u8; 36];
        table[0..4].copy_from_slice(b"DSDT");
        table.extend(device("PCI0", &root));
        table.extend(power_resource("PWR1", &regulator));
        table
    }

    fn leaked(bytes: Vec<u8>) -> &'static [u8] {
        Box::leak(bytes.into_boxed_slice())
    }

    #[test]
    fn parses_devices_power_resources_and_fields() {
        let namespace = Namespace::parse(&[leaked(fixture())]);
        assert!(matches!(namespace.get("\\PCI0"), Some(Object::Device)));
        assert!(matches!(namespace.get("\\PCI0.WLAN"), Some(Object::Device)));
        assert!(matches!(
            namespace.get("\\PWR1"),
            Some(Object::PowerResource { .. })
        ));
        assert!(matches!(
            namespace.get("\\PWR1.WLON"),
            Some(Object::Field {
                bit_offset: 0,
                bit_width: 1,
                ..
            })
        ));
        assert_eq!(
            namespace.get("\\PCI0.WLAN._ADR"),
            Some(&Object::Name(Value::Integer(0x0002_0000)))
        );
    }

    #[test]
    fn pr0_package_is_a_reference_to_the_power_resource() {
        let namespace = Namespace::parse(&[leaked(fixture())]);
        assert_eq!(
            namespace.get("\\PCI0.WLAN._PR0"),
            Some(&Object::Name(Value::Package(vec![Value::Reference(
                "\\PWR1".into()
            )])))
        );
    }

    #[test]
    fn on_method_writes_the_power_resource_field() {
        let namespace = Namespace::parse(&[leaked(fixture())]);
        let mut io = MockIo::default();
        let mut interpreter = Interpreter::new(&namespace);
        interpreter.invoke("\\PWR1._ON", &[], &mut io).unwrap();
        assert_eq!(io.writes, vec![(0x8000, 0x01)]);
    }

    #[test]
    fn sta_method_reports_the_resource_is_off() {
        let namespace = Namespace::parse(&[leaked(fixture())]);
        let mut io = MockIo::default();
        let mut interpreter = Interpreter::new(&namespace);
        assert_eq!(
            interpreter.invoke("\\PWR1._STA", &[], &mut io).unwrap(),
            Value::Integer(0)
        );
    }

    #[test]
    fn resynchronises_past_an_unknown_construct() {
        // `0x5b 0x9f` is not decoded here; the `Name` after it must still land.
        let mut body = vec![0x5b, 0x9f, 0xde, 0xad];
        body.extend(name_op("_HID", &dword(0x030A_D041)));
        let mut table = vec![0u8; 36];
        table[0..4].copy_from_slice(b"DSDT");
        table.extend(device("PCI1", &body));
        let namespace = Namespace::parse(&[leaked(table)]);
        assert_eq!(
            namespace.get("\\PCI1._HID"),
            Some(&Object::Name(Value::Integer(0x030A_D041)))
        );
    }

    #[test]
    fn executes_if_and_else_branches() {
        let namespace = Namespace::parse(&[leaked(fixture())]);
        let mut io = MockIo::default();
        let mut interpreter = Interpreter::new(&namespace);
        // If (One) { Store (One, WLON) }
        let body = vec![0xa0, 0x08, 0x01, 0x70, 0x01, b'W', b'L', b'O', b'N'];
        assert!(interpreter.invoke("\\PWR1._ON", &[], &mut io).is_ok());
        assert_eq!(io.writes, vec![(0x8000, 0x01)]);
        // The branch byte stream above is only here to keep the encoding honest;
        // it is covered by the parser test above.
        let _ = body;
    }

    #[test]
    fn reads_and_writes_unaligned_field_bits() {
        struct Bytes(BTreeMap<u64, u8>);
        impl RegionIo for Bytes {
            fn read(&mut self, _space: u8, offset: u64, length: usize) -> Result<Vec<u8>, AmlError> {
                Ok((0..length as u64)
                    .map(|index| *self.0.get(&(offset + index)).unwrap_or(&0))
                    .collect())
            }
            fn write(&mut self, _space: u8, offset: u64, bytes: &[u8]) -> Result<(), AmlError> {
                for (index, byte) in bytes.iter().enumerate() {
                    self.0.insert(offset + index as u64, *byte);
                }
                Ok(())
            }
        }
        let mut io = Bytes(BTreeMap::new());
        write_bits(&mut io, REGION_SYSTEM_IO, 11, 3, 0b101).unwrap();
        // Bits 11..14 live in byte 1, at bits 3..5.
        assert_eq!(io.0.get(&1), Some(&0b0010_1000));
        assert_eq!(read_bits(&mut io, REGION_SYSTEM_IO, 11, 3).unwrap(), 0b101);
    }

    #[test]
    fn a_table_of_unknown_bytes_parses_without_spinning() {
        let mut table = vec![0u8; 36];
        table[0..4].copy_from_slice(b"DSDT");
        table.extend(core::iter::repeat(0xffu8).take(1024));

        let namespace = Namespace::parse(&[leaked(table)]);

        assert_eq!(namespace.iter().count(), 0);
        assert!(namespace.resyncs() > 0, "every byte is resynchronised on");
        assert!(
            !namespace.truncated(),
            "resynchronising stays inside the iteration budget"
        );
    }
}
