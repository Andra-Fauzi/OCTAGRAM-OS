//! `xrustc` — the OCTAGRAM native Rust-subset compiler (M4 stage 0).
//!
//! M4 asks for a Rust compile step that runs inside OCTAGRAM instead of a host
//! `rustc`. A full `rustc` port needs `std`, LLVM and a dynamic loader, which do
//! not exist here yet, so stage 0 starts at the other end of the pipeline: a
//! frontend for a documented Rust subset that lowers to the same assembly
//! dialect `crate::toolchain` already assembles and links. Parsing, code
//! generation, instruction encoding, relocation and ELF writing therefore all
//! happen inside the kernel; nothing is cross-compiled on the host.
//!
//! Supported subset (everything else is rejected with a line number):
//!
//! - `fn` items with up to six `i64`/`u8`/`bool`/pointer parameters,
//!   `-> T` return types, recursion and forward calls.
//! - `let`/`let mut` bindings with an initial value, assignment, `return`,
//!   `if`/`else if`/`else`, `while`, `break`, and expression statements.
//! - `i64` values throughout (`u8`/`bool` are accepted in type positions and
//!   represented as `i64`), `true`/`false`, and `*const u8` string literals.
//! - `+ - *`, comparisons, `&&`, `||`, `!`, unary `-`, `as` casts.
//! - Intrinsics `putstr("literal")` and `putchar(value)` writing to stdout.
//!
//! Not supported (rejected, never miscompiled): `/`, `%`, arrays, structs,
//! `match`, closures, traits, generics, `extern` blocks, `core`/`alloc`, and
//! type checking beyond name resolution.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Registers used for the first six arguments of a call (SysV AMD64 order).
const ARGUMENT_REGISTERS: [&str; 6] = ["%rdi", "%rsi", "%rdx", "%rcx", "%r8", "%r9"];
const INTRINSICS: [&str; 2] = ["putstr", "putchar"];

/// A rejected source program, with the 1-based line the parser or code
/// generator stopped at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub line: usize,
    pub message: String,
}

impl CompileError {
    fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
}

impl core::fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "line {}: {}", self.line, self.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Ident(String),
    Int(i64),
    Str(Vec<u8>),
    Punct(&'static str),
    Eof,
}

static EOF_TOKEN: Token = Token::Eof;

fn tokenize(source: &str) -> Result<Vec<(Token, usize)>, CompileError> {
    const PUNCTUATION: [&str; 2] = ["->", "=="];
    const TWO_CHARACTER: [&str; 5] = ["!=", "<=", ">=", "&&", "||"];
    const ONE_CHARACTER: [&str; 20] = [
        "(", ")", "{", "}", "[", "]", ",", ":", ";", "=", "+", "-", "*", "/", "%", "<", ">", "!",
        "&", "|",
    ];

    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut line = 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'\n' {
            line += 1;
            index += 1;
            continue;
        }
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = index;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            tokens.push((Token::Ident(String::from(&source[start..index])), line));
            continue;
        }
        if byte.is_ascii_digit() {
            let start = index;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            let text = source[start..index].replace('_', "");
            let value = if let Some(hex) = text.strip_prefix("0x") {
                i64::from_str_radix(hex, 16).ok()
            } else {
                text.parse::<i64>().ok()
            };
            let value = value.ok_or_else(|| CompileError::new(line, "invalid integer literal"))?;
            tokens.push((Token::Int(value), line));
            continue;
        }
        if byte == b'"' {
            index += 1;
            let mut value = Vec::new();
            loop {
                let Some(&next) = bytes.get(index) else {
                    return Err(CompileError::new(line, "unterminated string literal"));
                };
                index += 1;
                match next {
                    b'"' => break,
                    b'\n' => return Err(CompileError::new(line, "unterminated string literal")),
                    b'\\' => {
                        let Some(&escaped) = bytes.get(index) else {
                            return Err(CompileError::new(line, "unterminated string literal"));
                        };
                        index += 1;
                        let decoded = match escaped {
                            b'n' => b'\n',
                            b't' => b'\t',
                            b'r' => b'\r',
                            b'0' => 0,
                            b'\\' => b'\\',
                            b'"' => b'"',
                            b'x' => {
                                let Some(high) = bytes.get(index) else {
                                    return Err(CompileError::new(line, "short \\x escape"));
                                };
                                let Some(low) = bytes.get(index + 1) else {
                                    return Err(CompileError::new(line, "short \\x escape"));
                                };
                                let digits = [*high, *low];
                                let digits = core::str::from_utf8(&digits)
                                    .map_err(|_| CompileError::new(line, "invalid \\x escape"))?;
                                index += 2;
                                u8::from_str_radix(digits, 16)
                                    .map_err(|_| CompileError::new(line, "invalid \\x escape"))?
                            }
                            _ => return Err(CompileError::new(line, "unsupported string escape")),
                        };
                        value.push(decoded);
                    }
                    other => value.push(other),
                }
            }
            tokens.push((Token::Str(value), line));
            continue;
        }

        let rest = &source[index..];
        let matched = PUNCTUATION
            .iter()
            .chain(TWO_CHARACTER.iter())
            .find(|punct| rest.starts_with(**punct))
            .copied()
            .or_else(|| {
                ONE_CHARACTER
                    .iter()
                    .find(|punct| rest.starts_with(**punct))
                    .copied()
            });
        match matched {
            Some(punct) => {
                tokens.push((Token::Punct(punct), line));
                index += punct.len();
            }
            None => {
                return Err(CompileError::new(
                    line,
                    format!("unexpected character `{}`", &rest[..1]),
                ));
            }
        }
    }
    Ok(tokens)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnOp {
    Negate,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinOp {
    Add,
    Subtract,
    Multiply,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Expr {
    Int(i64),
    Bool(bool),
    Str(Vec<u8>),
    Var {
        line: usize,
        name: String,
    },
    Call {
        line: usize,
        name: String,
        arguments: Vec<Expr>,
    },
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Stmt {
    Let {
        line: usize,
        name: String,
        value: Expr,
    },
    Assign {
        line: usize,
        name: String,
        value: Expr,
    },
    Return(Option<Expr>),
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    Break(usize),
    Expression(Expr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Function {
    name: String,
    parameters: Vec<String>,
    body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Program {
    functions: Vec<Function>,
}

struct Parser<'a> {
    tokens: &'a [(Token, usize)],
    position: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [(Token, usize)]) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    fn line(&self) -> usize {
        self.tokens
            .get(self.position)
            .map(|(_, line)| *line)
            .or_else(|| self.tokens.last().map(|(_, line)| *line))
            .unwrap_or(1)
    }

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.position)
            .map(|(token, _)| token)
            .unwrap_or(&EOF_TOKEN)
    }

    fn peek_at(&self, offset: usize) -> &Token {
        self.tokens
            .get(self.position + offset)
            .map(|(token, _)| token)
            .unwrap_or(&EOF_TOKEN)
    }

    fn advance(&mut self) -> Token {
        let token = self.peek().clone();
        if self.position < self.tokens.len() {
            self.position += 1;
        }
        token
    }

    fn error(&self, message: impl Into<String>) -> CompileError {
        CompileError::new(self.line(), message)
    }

    fn is_punct(&self, punct: &str) -> bool {
        matches!(self.peek(), Token::Punct(value) if *value == punct)
    }

    fn eat_punct(&mut self, punct: &str) -> bool {
        if self.is_punct(punct) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect_punct(&mut self, punct: &str) -> Result<(), CompileError> {
        if self.eat_punct(punct) {
            Ok(())
        } else {
            Err(self.error(format!("expected `{punct}`")))
        }
    }

    fn is_keyword(&self, keyword: &str) -> bool {
        matches!(self.peek(), Token::Ident(name) if name == keyword)
    }

    fn eat_keyword(&mut self, keyword: &str) -> bool {
        if self.is_keyword(keyword) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect_ident(&mut self) -> Result<String, CompileError> {
        let line = self.line();
        match self.advance() {
            Token::Ident(name) => Ok(name),
            _ => Err(CompileError::new(line, "expected an identifier")),
        }
    }

    /// Consume a type annotation. Stage 0 only needs to accept the names the
    /// subset documents; values are all `i64` once compiled.
    fn skip_type(&mut self) -> Result<(), CompileError> {
        const TYPE_NAMES: [&str; 10] = [
            "i64", "u8", "u16", "u32", "u64", "usize", "isize", "bool", "str", "const",
        ];
        let start = self.position;
        loop {
            let token = self.peek().clone();
            match token {
                Token::Punct(",") | Token::Punct(")") | Token::Punct("=") | Token::Punct("{")
                | Token::Punct(";") => break,
                Token::Punct("*") | Token::Punct("&") | Token::Punct("(") | Token::Punct("mut") => {
                    self.position += 1;
                }
                Token::Ident(name) if TYPE_NAMES.contains(&name.as_str()) => {
                    self.position += 1;
                }
                Token::Ident(name) => {
                    return Err(self.error(format!("unsupported type `{name}`")));
                }
                Token::Punct(punct) => {
                    return Err(self.error(format!("unsupported type syntax `{punct}`")));
                }
                _ => return Err(self.error("expected a type")),
            }
        }
        if self.position == start {
            return Err(self.error("expected a type"));
        }
        Ok(())
    }

    fn parse_program(&mut self) -> Result<Program, CompileError> {
        let mut functions = Vec::new();
        while !matches!(self.peek(), Token::Eof) {
            if !self.eat_keyword("fn") {
                return Err(self.error("expected `fn`: xrustc stage 0 compiles free functions only"));
            }
            let line = self.line();
            let name = self.expect_ident()?;
            self.expect_punct("(")?;
            let mut parameters = Vec::new();
            if !self.eat_punct(")") {
                loop {
                    let parameter = self.expect_ident()?;
                    self.expect_punct(":")?;
                    self.skip_type()?;
                    parameters.push(parameter);
                    if self.eat_punct(",") {
                        continue;
                    }
                    self.expect_punct(")")?;
                    break;
                }
            }
            if parameters.len() > ARGUMENT_REGISTERS.len() {
                return Err(CompileError::new(
                    line,
                    format!(
                        "`{name}` has more than {} parameters, which stage 0 cannot pass",
                        ARGUMENT_REGISTERS.len()
                    ),
                ));
            }
            if self.eat_punct("->") {
                self.skip_type()?;
            }
            let body = self.parse_block()?;
            functions.push(Function {
                name,
                parameters,
                body,
            });
        }
        if functions.is_empty() {
            return Err(self.error("no functions to compile"));
        }
        Ok(Program { functions })
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, CompileError> {
        self.expect_punct("{")?;
        let mut statements = Vec::new();
        while !self.eat_punct("}") {
            if matches!(self.peek(), Token::Eof) {
                return Err(self.error("unterminated block"));
            }
            statements.push(self.parse_statement()?);
        }
        Ok(statements)
    }

    fn parse_statement(&mut self) -> Result<Stmt, CompileError> {
        let line = self.line();
        if self.eat_keyword("let") {
            self.eat_keyword("mut");
            let name = self.expect_ident()?;
            if self.eat_punct(":") {
                self.skip_type()?;
            }
            self.expect_punct("=")?;
            let value = self.parse_expression()?;
            self.expect_punct(";")?;
            return Ok(Stmt::Let { line, name, value });
        }
        if self.eat_keyword("return") {
            if self.eat_punct(";") {
                return Ok(Stmt::Return(None));
            }
            let value = self.parse_expression()?;
            self.expect_punct(";")?;
            return Ok(Stmt::Return(Some(value)));
        }
        if self.eat_keyword("if") {
            return self.parse_if();
        }
        if self.eat_keyword("while") {
            let condition = self.parse_expression()?;
            let body = self.parse_block()?;
            return Ok(Stmt::While { condition, body });
        }
        if self.eat_keyword("break") {
            self.expect_punct(";")?;
            return Ok(Stmt::Break(line));
        }
        if let (Token::Ident(name), Token::Punct("=")) = (self.peek_at(0), self.peek_at(1)) {
            let name = name.clone();
            self.position += 2;
            let value = self.parse_expression()?;
            self.expect_punct(";")?;
            return Ok(Stmt::Assign { line, name, value });
        }
        let expression = self.parse_expression()?;
        self.expect_punct(";")?;
        Ok(Stmt::Expression(expression))
    }

    fn parse_if(&mut self) -> Result<Stmt, CompileError> {
        let condition = self.parse_expression()?;
        let then_body = self.parse_block()?;
        let mut else_body = Vec::new();
        if self.eat_keyword("else") {
            if self.eat_keyword("if") {
                else_body.push(self.parse_if()?);
            } else {
                else_body = self.parse_block()?;
            }
        }
        Ok(Stmt::If {
            condition,
            then_body,
            else_body,
        })
    }

    fn parse_expression(&mut self) -> Result<Expr, CompileError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_and()?;
        while self.eat_punct("||") {
            let right = self.parse_and()?;
            left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_equality()?;
        while self.eat_punct("&&") {
            let right = self.parse_equality()?;
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_relational()?;
        loop {
            let operation = if self.eat_punct("==") {
                BinOp::Equal
            } else if self.eat_punct("!=") {
                BinOp::NotEqual
            } else {
                break;
            };
            let right = self.parse_relational()?;
            left = Expr::Binary(operation, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_relational(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_additive()?;
        loop {
            let operation = if self.eat_punct("<=") {
                BinOp::LessEqual
            } else if self.eat_punct(">=") {
                BinOp::GreaterEqual
            } else if self.eat_punct("<") {
                BinOp::Less
            } else if self.eat_punct(">") {
                BinOp::Greater
            } else {
                break;
            };
            let right = self.parse_additive()?;
            left = Expr::Binary(operation, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let operation = if self.eat_punct("+") {
                BinOp::Add
            } else if self.eat_punct("-") {
                BinOp::Subtract
            } else {
                break;
            };
            let right = self.parse_multiplicative()?;
            left = Expr::Binary(operation, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_unary()?;
        loop {
            if self.is_punct("/") || self.is_punct("%") {
                let punct = if self.is_punct("/") { "/" } else { "%" };
                return Err(self.error(format!(
                    "`{punct}` is not supported by xrustc stage 0: the native backend has no divide instruction"
                )));
            }
            if !self.eat_punct("*") {
                break;
            }
            let right = self.parse_unary()?;
            left = Expr::Binary(BinOp::Multiply, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, CompileError> {
        if self.eat_punct("-") {
            let operand = self.parse_unary()?;
            return Ok(Expr::Unary(UnOp::Negate, Box::new(operand)));
        }
        if self.eat_punct("!") {
            let operand = self.parse_unary()?;
            return Ok(Expr::Unary(UnOp::Not, Box::new(operand)));
        }
        let mut value = self.parse_primary()?;
        while self.eat_keyword("as") {
            self.skip_type()?;
        }
        Ok(value)
    }

    fn parse_primary(&mut self) -> Result<Expr, CompileError> {
        let line = self.line();
        match self.advance() {
            Token::Int(value) => Ok(Expr::Int(value)),
            Token::Str(bytes) => Ok(Expr::Str(bytes)),
            Token::Ident(name) => {
                match name.as_str() {
                    "true" => return Ok(Expr::Bool(true)),
                    "false" => return Ok(Expr::Bool(false)),
                    _ => {}
                }
                if self.eat_punct("(") {
                    let mut arguments = Vec::new();
                    if !self.eat_punct(")") {
                        loop {
                            arguments.push(self.parse_expression()?);
                            if self.eat_punct(",") {
                                continue;
                            }
                            self.expect_punct(")")?;
                            break;
                        }
                    }
                    Ok(Expr::Call {
                        line,
                        name,
                        arguments,
                    })
                } else {
                    Ok(Expr::Var { line, name })
                }
            }
            Token::Punct("(") => {
                let inner = self.parse_expression()?;
                self.expect_punct(")")?;
                Ok(inner)
            }
            _ => Err(CompileError::new(line, "expected an expression")),
        }
    }
}

/// Translate source text into the assembly dialect `crate::toolchain::assemble`
/// accepts. The returned text already contains `_start`, so assembling and
/// linking it yields a runnable static ELF.
pub fn compile_to_assembly(source: &str) -> Result<String, CompileError> {
    let tokens = tokenize(source)?;
    let program = Parser::new(&tokens).parse_program()?;
    Generator::new(&program)?.render()
}

struct Generator {
    output: String,
    program: Program,
    functions: BTreeMap<String, usize>,
    strings: Vec<Vec<u8>>,
    labels: usize,
    locals: BTreeMap<String, usize>,
    frame_words: usize,
    loop_ends: Vec<String>,
    return_label: String,
}

impl Generator {
    fn new(program: &Program) -> Result<Self, CompileError> {
        let mut functions = BTreeMap::new();
        for function in &program.functions {
            if INTRINSICS.contains(&function.name.as_str()) {
                return Err(CompileError::new(
                    1,
                    format!("`{}` is a built-in intrinsic and cannot be redefined", function.name),
                ));
            }
            if functions
                .insert(function.name.clone(), function.parameters.len())
                .is_some()
            {
                return Err(CompileError::new(
                    1,
                    format!("function `{}` is defined twice", function.name),
                ));
            }
        }
        if !functions.contains_key("main") {
            return Err(CompileError::new(1, "missing `fn main()`"));
        }
        Ok(Self {
            output: String::new(),
            program: program.clone(),
            functions,
            strings: Vec::new(),
            labels: 0,
            locals: BTreeMap::new(),
            frame_words: 0,
            loop_ends: Vec::new(),
            return_label: String::new(),
        })
    }

    fn render(mut self) -> Result<String, CompileError> {
        // The kernel's `_start` is written here instead of relying on the
        // assembler's implicit startup, so the exit path stays visible next to
        // the generated code.
        self.emit(".global _start");
        self.emit(".text");
        self.emit("_start:");
        self.emit("call main");
        self.emit("mov %rax, %rdi");
        self.emit("mov $0, %rax");
        self.emit("int $0x80");

        let program = self.program.clone();
        for function in &program.functions {
            self.emit_function(function)?;
        }
        self.emit_string_data();
        Ok(self.output)
    }

    fn emit_string_data(&mut self) {
        if self.strings.is_empty() {
            return;
        }
        self.emit(".rodata");
        let strings = core::mem::take(&mut self.strings);
        for (index, bytes) in strings.iter().enumerate() {
            self.emit(&format!(".Lxr_string{index}:"));
            let mut directive = String::from(".byte ");
            for byte in bytes {
                directive.push_str(&format!("{byte},"));
            }
            directive.push('0');
            self.emit(&directive);
        }
    }

    fn emit_function(&mut self, function: &Function) -> Result<(), CompileError> {
        self.locals.clear();
        self.loop_ends.clear();
        self.frame_words = function.parameters.len();
        for (index, name) in function.parameters.iter().enumerate() {
            self.locals.insert(name.clone(), (index + 1) * 8);
        }
        self.collect_locals(&function.body);
        self.return_label = format!(".Lxr_return_{}", function.name);
        let frame = if self.frame_words == 0 {
            0
        } else {
            (self.frame_words * 8).div_ceil(16) * 16
        };

        self.emit(&format!("{}:", function.name));
        self.emit("push %rbp");
        self.emit("mov %rsp, %rbp");
        if frame > 0 {
            self.emit(&format!("sub ${frame}, %rsp"));
        }
        for index in 0..function.parameters.len() {
            self.emit(&format!(
                "mov {}, -{}(%rbp)",
                ARGUMENT_REGISTERS[index],
                (index + 1) * 8
            ));
        }
        self.emit_statements(&function.body)?;
        self.emit("mov $0, %rax");
        let return_label = self.return_label.clone();
        self.emit(&format!("{return_label}:"));
        self.emit("mov %rbp, %rsp");
        self.emit("pop %rbp");
        self.emit("ret");
        Ok(())
    }

    fn collect_locals(&mut self, statements: &[Stmt]) {
        for statement in statements {
            match statement {
                Stmt::Let { name, .. } => {
                    if !self.locals.contains_key(name) {
                        self.frame_words += 1;
                        self.locals.insert(name.clone(), self.frame_words * 8);
                    }
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    self.collect_locals(then_body);
                    self.collect_locals(else_body);
                }
                Stmt::While { body, .. } => self.collect_locals(body),
                _ => {}
            }
        }
    }

    fn emit_statements(&mut self, statements: &[Stmt]) -> Result<(), CompileError> {
        for statement in statements {
            self.emit_statement(statement)?;
        }
        Ok(())
    }

    fn emit_statement(&mut self, statement: &Stmt) -> Result<(), CompileError> {
        match statement {
            Stmt::Let { line, name, value } | Stmt::Assign { line, name, value } => {
                let Some(offset) = self.locals.get(name).copied() else {
                    return Err(CompileError::new(
                        *line,
                        format!("unknown variable `{name}`"),
                    ));
                };
                self.emit_expression(value)?;
                self.emit(&format!("mov %rax, -{offset}(%rbp)"));
            }
            Stmt::Return(value) => {
                match value {
                    Some(value) => self.emit_expression(value)?,
                    None => self.emit("mov $0, %rax"),
                }
                let label = self.return_label.clone();
                self.emit(&format!("jmp {label}"));
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                let else_label = self.next_label("else");
                let end_label = if else_body.is_empty() {
                    None
                } else {
                    Some(self.next_label("if_end"))
                };
                self.emit_expression(condition)?;
                self.emit("test %rax, %rax");
                self.emit(&format!("je {else_label}"));
                self.emit_statements(then_body)?;
                if let Some(end_label) = &end_label {
                    self.emit(&format!("jmp {end_label}"));
                }
                self.emit(&format!("{else_label}:"));
                self.emit_statements(else_body)?;
                if let Some(end_label) = &end_label {
                    self.emit(&format!("{end_label}:"));
                }
            }
            Stmt::While { condition, body } => {
                let start_label = self.next_label("while");
                let end_label = self.next_label("while_end");
                self.emit(&format!("{start_label}:"));
                self.emit_expression(condition)?;
                self.emit("test %rax, %rax");
                self.emit(&format!("je {end_label}"));
                self.loop_ends.push(end_label.clone());
                let result = self.emit_statements(body);
                self.loop_ends.pop();
                result?;
                self.emit(&format!("jmp {start_label}"));
                self.emit(&format!("{end_label}:"));
            }
            Stmt::Break(line) => {
                let Some(end_label) = self.loop_ends.last().cloned() else {
                    return Err(CompileError::new(*line, "`break` outside a loop"));
                };
                self.emit(&format!("jmp {end_label}"));
            }
            Stmt::Expression(expression) => self.emit_expression(expression)?,
        }
        Ok(())
    }

    fn emit_expression(&mut self, expression: &Expr) -> Result<(), CompileError> {
        match expression {
            Expr::Int(value) => self.emit(&format!("mov ${value}, %rax")),
            Expr::Bool(value) => self.emit(&format!("mov ${}, %rax", i64::from(*value))),
            Expr::Str(bytes) => {
                let index = self.intern(bytes);
                self.emit(&format!("lea .Lxr_string{index}(%rip), %rax"));
            }
            Expr::Var { line, name } => {
                let Some(offset) = self.locals.get(name).copied() else {
                    return Err(CompileError::new(
                        *line,
                        format!("unknown variable `{name}`"),
                    ));
                };
                self.emit(&format!("mov -{offset}(%rbp), %rax"));
            }
            Expr::Unary(UnOp::Negate, operand) => {
                self.emit_expression(operand)?;
                self.emit("push %rax");
                self.emit("mov $0, %rax");
                self.emit("pop %rdi");
                self.emit("sub %rdi, %rax");
            }
            Expr::Unary(UnOp::Not, operand) => {
                self.emit_expression(operand)?;
                self.emit("test %rax, %rax");
                self.emit("sete %al");
                self.emit("movzb %al, %rax");
            }
            Expr::Binary(operation, left, right) => {
                self.emit_binary(*operation, left, right)?
            }
            Expr::Call {
                line,
                name,
                arguments,
            } => self.emit_call(*line, name, arguments)?,
        }
        Ok(())
    }

    fn emit_binary(
        &mut self,
        operation: BinOp,
        left: &Expr,
        right: &Expr,
    ) -> Result<(), CompileError> {
        match operation {
            BinOp::And => {
                let false_label = self.next_label("and_false");
                let end_label = self.next_label("and_end");
                self.emit_expression(left)?;
                self.emit("test %rax, %rax");
                self.emit(&format!("je {false_label}"));
                self.emit_expression(right)?;
                self.emit("test %rax, %rax");
                self.emit(&format!("je {false_label}"));
                self.emit("mov $1, %rax");
                self.emit(&format!("jmp {end_label}"));
                self.emit(&format!("{false_label}:"));
                self.emit("mov $0, %rax");
                self.emit(&format!("{end_label}:"));
            }
            BinOp::Or => {
                let true_label = self.next_label("or_true");
                let end_label = self.next_label("or_end");
                self.emit_expression(left)?;
                self.emit("test %rax, %rax");
                self.emit(&format!("jne {true_label}"));
                self.emit_expression(right)?;
                self.emit("test %rax, %rax");
                self.emit(&format!("jne {true_label}"));
                self.emit("mov $0, %rax");
                self.emit(&format!("jmp {end_label}"));
                self.emit(&format!("{true_label}:"));
                self.emit("mov $1, %rax");
                self.emit(&format!("{end_label}:"));
            }
            BinOp::Add | BinOp::Subtract | BinOp::Multiply => {
                self.emit_expression(left)?;
                self.emit("push %rax");
                self.emit_expression(right)?;
                self.emit("pop %rdi");
                match operation {
                    BinOp::Add => self.emit("add %rdi, %rax"),
                    BinOp::Subtract => {
                        self.emit("sub %rax, %rdi");
                        self.emit("mov %rdi, %rax");
                    }
                    _ => self.emit("imul %rdi, %rax"),
                }
            }
            _ => {
                self.emit_expression(left)?;
                self.emit("push %rax");
                self.emit_expression(right)?;
                self.emit("pop %rdi");
                self.emit("cmp %rax, %rdi");
                let set = match operation {
                    BinOp::Less => "setl",
                    BinOp::LessEqual => "setle",
                    BinOp::Greater => "setg",
                    BinOp::GreaterEqual => "setge",
                    BinOp::Equal => "sete",
                    _ => "setne",
                };
                self.emit(&format!("{set} %al"));
                self.emit("movzb %al, %rax");
            }
        }
        Ok(())
    }

    fn emit_call(
        &mut self,
        line: usize,
        name: &str,
        arguments: &[Expr],
    ) -> Result<(), CompileError> {
        if name == "putstr" {
            if arguments.len() != 1 {
                return Err(CompileError::new(
                    line,
                    "putstr takes exactly one string literal",
                ));
            }
            let Expr::Str(bytes) = &arguments[0] else {
                return Err(CompileError::new(
                    line,
                    "putstr stage 0 needs a string literal argument",
                ));
            };
            let index = self.intern(bytes);
            let length = bytes.len();
            self.emit("mov $1, %rax");
            self.emit("mov $1, %rdi");
            self.emit(&format!("lea .Lxr_string{index}(%rip), %rsi"));
            self.emit(&format!("mov ${length}, %rdx"));
            self.emit("int $0x80");
            return Ok(());
        }
        if name == "putchar" {
            if arguments.len() != 1 {
                return Err(CompileError::new(line, "putchar takes exactly one value"));
            }
            self.emit_expression(&arguments[0])?;
            self.emit("push %rax");
            self.emit("mov $1, %rax");
            self.emit("mov $1, %rdi");
            self.emit("mov %rsp, %rsi");
            self.emit("mov $1, %rdx");
            self.emit("int $0x80");
            self.emit("pop %rdi");
            return Ok(());
        }

        let Some(&arity) = self.functions.get(name) else {
            return Err(CompileError::new(
                line,
                format!("unknown function `{name}`"),
            ));
        };
        if arguments.len() != arity {
            return Err(CompileError::new(
                line,
                format!(
                    "`{name}` expects {arity} arguments but received {}",
                    arguments.len()
                ),
            ));
        }
        for argument in arguments {
            self.emit_expression(argument)?;
            self.emit("push %rax");
        }
        for index in (0..arguments.len()).rev() {
            self.emit(&format!("pop {}", ARGUMENT_REGISTERS[index]));
        }
        self.emit(&format!("call {name}"));
        Ok(())
    }

    fn emit(&mut self, text: &str) {
        self.output.push_str(text);
        self.output.push('\n');
    }

    fn next_label(&mut self, prefix: &str) -> String {
        let id = self.labels;
        self.labels += 1;
        format!(".Lxr_{prefix}{id}")
    }

    fn intern(&mut self, bytes: &[u8]) -> usize {
        if let Some(index) = self
            .strings
            .iter()
            .position(|existing| existing.as_slice() == bytes)
        {
            return index;
        }
        self.strings.push(bytes.to_vec());
        self.strings.len() - 1
    }
}

/// Assembly text plus the static ELF the in-kernel assembler and linker built
/// from it.
pub struct CompiledProgram {
    pub assembly: String,
    pub executable: Vec<u8>,
}

/// Compile source to a runnable static ELF without leaving the kernel.
pub fn compile_to_elf(source: &str) -> Result<CompiledProgram, CompileError> {
    let assembly = compile_to_assembly(source)?;
    let object = crate::toolchain::assemble(assembly.as_bytes()).map_err(|error| {
        CompileError::new(
            1,
            format!("native assembler rejected generated code: {error:?}"),
        )
    })?;
    let executable = crate::toolchain::link(&object).map_err(|error| {
        CompileError::new(
            1,
            format!("native linker rejected generated code: {error:?}"),
        )
    })?;
    Ok(CompiledProgram {
        assembly,
        executable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECURSIVE_PROGRAM: &str = r#"
// Sum the first ten Fibonacci numbers with recursion and a loop.
fn fib(n: i64) -> i64 {
    if n < 2 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

fn main() -> i64 {
    let mut index: i64 = 0;
    let mut sum: i64 = 0;
    while index < 10 {
        sum = sum + fib(index);
        index = index + 1;
    }
    putstr("xrustc fib sum: ");
    putchar(48 + sum);
    putchar(10);
    return 0;
}
"#;

    #[test]
    fn compiles_recursion_and_loops_into_a_linked_elf() {
        let compiled = compile_to_elf(RECURSIVE_PROGRAM).unwrap();
        assert!(compiled.assembly.starts_with(".global _start\n.text\n_start:\ncall main\n"));
        assert!(compiled.assembly.contains("call fib"));
        assert!(compiled.assembly.contains(".Lxr_string0:"));
        assert_eq!(&compiled.executable[..4], b"\x7fELF");
        let entry = u64::from_le_bytes(compiled.executable[24..32].try_into().unwrap());
        assert_eq!(entry, 0x400000);
    }

    #[test]
    fn short_circuit_operators_keep_the_second_operand_lazy() {
        let compiled = compile_to_elf(
            "fn main() -> i64 {\n    if 0 != 0 && missing() {\n        return 1;\n    }\n    return 0;\n}\n",
        );
        // The frontend rejects the unknown call before short-circuiting it, so
        // the failure proves the call is resolved at compile time.
        assert!(
            compiled
                .err()
                .expect("unknown call must be rejected")
                .message
                .contains("unknown function `missing`")
        );
    }

    #[test]
    fn rejects_division_with_the_source_line() {
        let error =
            compile_to_assembly("fn main() -> i64 {\n    let value: i64 = 4 / 2;\n    return value;\n}\n")
                .unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains("`/` is not supported"));
    }

    #[test]
    fn rejects_putstr_without_a_string_literal() {
        let error = compile_to_assembly(
            "fn main() -> i64 {\n    let value: i64 = 1;\n    putstr(value);\n    return 0;\n}\n",
        )
        .unwrap_err();
        assert_eq!(error.line, 3);
        assert!(error.message.contains("string literal"));
    }

    #[test]
    fn reports_unknown_variables_and_argument_counts() {
        let missing_variable = compile_to_assembly("fn main() -> i64 {\n    return missing;\n}\n")
            .unwrap_err();
        assert_eq!(missing_variable.line, 2);
        assert!(missing_variable.message.contains("unknown variable `missing`"));

        let wrong_arguments = compile_to_assembly(
            "fn twice(value: i64) -> i64 {\n    return value + value;\n}\nfn main() -> i64 {\n    return twice();\n}\n",
        )
        .unwrap_err();
        assert_eq!(wrong_arguments.line, 5);
        assert!(wrong_arguments.message.contains("expects 1 arguments but received 0"));
    }

    #[test]
    fn rejects_unsupported_types_and_missing_main() {
        let unsupported = compile_to_assembly("fn main() -> Vec {\n    return 0;\n}\n").unwrap_err();
        assert!(unsupported.message.contains("unsupported type `Vec`"));

        let no_main = compile_to_assembly("fn helper() -> i64 {\n    return 0;\n}\n").unwrap_err();
        assert!(no_main.message.contains("missing `fn main()`"));
    }

    #[test]
    fn rejects_break_outside_a_loop() {
        let error =
            compile_to_assembly("fn main() -> i64 {\n    break;\n    return 0;\n}\n").unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains("`break` outside a loop"));
    }
}
