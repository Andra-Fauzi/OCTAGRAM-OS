//! Loader for static ELF64 x86_64 programs and the minimal OCTAGRAM syscall ABI.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::arch::x86_64::{
    fpu::{self, State as FpuState},
    gdt,
    interrupt::{self, InterruptFrame, InterruptStackFrame},
};
use crate::fs::{self, DirectoryEntry, FsError, NodeType};
use crate::memory::{
    allocate_frame, free_frame, paging,
    paging::{PAGE_SIZE, PagingError},
};

const ELF_HEADER_SIZE: usize = 64;
const PROGRAM_HEADER_SIZE: usize = 56;
const MAX_PROGRAM_HEADERS: usize = 128;
const STACK_PAGES: u64 = 8;
const MAX_ARGUMENTS: usize = 64;
const MAX_ARGUMENT_BYTES: usize = 4096;
const MAX_ENVIRONMENT_VARIABLES: usize = 64;
const MAX_ENVIRONMENT_BYTES: usize = 4096;
const STACK_TOP: u64 = paging::USER_ADDRESS_MAX;
const HEAP_START: u64 = 0x1000_0000;
const HEAP_PAGES: u64 = 256;
const HEAP_INITIAL_END: u64 = HEAP_START + HEAP_PAGES * PAGE_SIZE;
const MMAP_START: u64 = 0x1_0000_0000;
const MMAP_END: u64 = STACK_TOP - (STACK_PAGES + 1) * PAGE_SIZE;
const MAX_USER_PATH: usize = 256;
const MAX_SYSCALL_IO: usize = 4096;
const MAX_OPEN_FILES: usize = 16;
const MAX_SCHEDULED_PROCESSES: usize = 16;
const PIPE_CAPACITY: usize = 4096;
const WAIT_ANY_PID: u64 = u64::MAX;
const PROCESS_KERNEL_STACK_SIZE: usize = 16 * 1024;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const SYSCALL_EXIT: u64 = 0;
const SYSCALL_WRITE: u64 = 1;
const SYSCALL_READ: u64 = 2;
const SYSCALL_OPEN: u64 = 3;
const SYSCALL_CLOSE: u64 = 4;
const SYSCALL_LSEEK: u64 = 5;
const SYSCALL_STAT: u64 = 6;
const SYSCALL_MKDIR: u64 = 7;
const SYSCALL_UNLINK: u64 = 8;
const SYSCALL_FTRUNCATE: u64 = 9;
const SYSCALL_BRK: u64 = 10;
const SYSCALL_GETPID: u64 = 11;
const SYSCALL_ISATTY: u64 = 12;
const SYSCALL_CHDIR: u64 = 13;
const SYSCALL_GETCWD: u64 = 14;
const SYSCALL_PUTPIXEL: u64 = 15;
const SYSCALL_MMAP: u64 = 16;
const SYSCALL_MUNMAP: u64 = 17;
const SYSCALL_MMAP_FILE: u64 = 18;
const SYSCALL_SPAWN: u64 = 19;
const SYSCALL_WAITPID: u64 = 20;
const SYSCALL_PIPE: u64 = 21;
const SYSCALL_DUP2: u64 = 22;
const SYSCALL_SETENVIRON: u64 = 23;
const SYSCALL_SPAWN_ENV: u64 = 24;
const SYSCALL_CHMOD: u64 = 25;
const SYSCALL_PRINTF: u64 = 26;
const SYSCALL_OPENDIR: u64 = 27;
const SYSCALL_READDIR: u64 = 28;

const PROT_READ: u64 = 1;
const PROT_WRITE: u64 = 2;
const PROT_EXEC: u64 = 4;

const O_RDONLY: u64 = 0;
const O_WRONLY: u64 = 1;
const O_RDWR: u64 = 2;
const O_ACCESS_MASK: u64 = 3;
const O_CREAT: u64 = 1 << 8;
const O_TRUNC: u64 = 1 << 9;
const O_APPEND: u64 = 1 << 10;

const EINTR: i32 = 4;
const E2BIG: i32 = 7;
const EIO: i32 = 5;
const ENOEXEC: i32 = 8;
const EBADF: i32 = 9;
const ECHILD: i32 = 10;
const EAGAIN: i32 = 11;
const ENOMEM: i32 = 12;
const EFAULT: i32 = 14;
const ENOENT: i32 = 2;
const EEXIST: i32 = 17;
const ENOTDIR: i32 = 20;
const EISDIR: i32 = 21;
const ENODEV: i32 = 19;
const EINVAL: i32 = 22;
const ESPIPE: i32 = 29;
const ERANGE: i32 = 34;
const EACCES: i32 = 13;
const EMFILE: i32 = 24;
const ENOTTY: i32 = 25;
const ENOSPC: i32 = 28;
const EROFS: i32 = 30;
const ENAMETOOLONG: i32 = 36;
const ENOSYS: i32 = 38;
const ENOTEMPTY: i32 = 39;
const ELOOP: i32 = 40;
const EILSEQ: i32 = 84;
const EPIPE: i32 = 32;
const EDEADLK: i32 = 35;
const EOVERFLOW: i32 = 75;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfError {
    InvalidHeader,
    UnsupportedFormat,
    DynamicLinkingUnsupported,
    OutOfBounds,
    InvalidSegment,
    WritableExecutableSegment,
    InvalidEntryPoint,
    AddressRange,
    OverlappingSegments,
    TooLarge,
    OutOfMemory,
    ProcessAlreadyRunning,
    InvalidArguments,
    ArgumentsTooLarge,
    SchedulerInvariant,
    PermissionDenied,
    FileSystem(FsError),
    Paging(PagingError),
}

impl From<FsError> for ElfError {
    fn from(error: FsError) -> Self {
        Self::FileSystem(error)
    }
}

impl From<PagingError> for ElfError {
    fn from(error: PagingError) -> Self {
        Self::Paging(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LoadSegment {
    file_offset: usize,
    virtual_address: u64,
    file_size: usize,
    memory_size: u64,
    flags: u32,
}

pub struct LoadedElf {
    entry: u64,
    user_stack: u64,
    heap_limit: u64,
    mmap_cursor: u64,
    environment: Vec<String>,
    mapped_pages: Vec<u64>,
    address_space: paging::AddressSpace,
}

#[derive(Clone)]
struct OpenFile {
    path: String,
    offset: usize,
    readable: bool,
    writable: bool,
    append: bool,
}

struct DirectoryStream {
    entries: Vec<DirectoryEntry>,
    next_entry: usize,
}

#[repr(C)]
struct UserDirent {
    inode: u64,
    offset: i64,
    record_length: u16,
    file_type: u8,
    name: [u8; 256],
}

struct Pipe {
    id: u64,
    buffer: Vec<u8>,
    head: usize,
    length: usize,
    readers: usize,
    writers: usize,
}

impl Pipe {
    fn write(&mut self, input: &[u8]) -> Result<usize, i32> {
        if self.readers == 0 {
            return Err(EPIPE);
        }
        let available = PIPE_CAPACITY - self.length;
        if available == 0 {
            return Err(EAGAIN);
        }
        let count = input.len().min(available);
        for byte in &input[..count] {
            let tail = (self.head + self.length) % PIPE_CAPACITY;
            self.buffer[tail] = *byte;
            self.length += 1;
        }
        Ok(count)
    }

    fn read(&mut self, output: &mut [u8]) -> Result<usize, i32> {
        if self.length == 0 {
            return if self.writers == 0 {
                Ok(0)
            } else {
                Err(EAGAIN)
            };
        }
        let count = output.len().min(self.length);
        for byte in &mut output[..count] {
            *byte = self.buffer[self.head];
            self.head = (self.head + 1) % PIPE_CAPACITY;
            self.length -= 1;
        }
        Ok(count)
    }
}

struct PipeEnd {
    pipe: Arc<Mutex<Pipe>>,
    readable: bool,
    writable: bool,
}

impl Clone for PipeEnd {
    fn clone(&self) -> Self {
        let mut pipe = self.pipe.lock();
        if self.readable {
            pipe.readers += 1;
        }
        if self.writable {
            pipe.writers += 1;
        }
        drop(pipe);
        Self {
            pipe: Arc::clone(&self.pipe),
            readable: self.readable,
            writable: self.writable,
        }
    }
}

impl Drop for PipeEnd {
    fn drop(&mut self) {
        let mut pipe = self.pipe.lock();
        if self.readable {
            pipe.readers -= 1;
        }
        if self.writable {
            pipe.writers -= 1;
        }
    }
}

#[derive(Clone)]
enum UserDescriptor {
    ConsoleInput,
    ConsoleOutput,
    ConsoleError,
    File(OpenFile),
    Directory(Arc<Mutex<DirectoryStream>>),
    Pipe(PipeEnd),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProcessWait {
    Child(u64),
    PipeReadable(u64),
    PipeWritable(u64),
}

struct UserProcess {
    active: bool,
    pid: u64,
    terminal: usize,
    descriptors: Vec<Option<UserDescriptor>>,
    cwd: String,
    environment: Vec<String>,
    program_break: u64,
    heap_mapped_end: u64,
    heap_limit: u64,
    mmap_cursor: u64,
    heap_pages: Vec<u64>,
    mmap_pages: Vec<u64>,
    fpu_state: FpuState,
}

impl UserProcess {
    const fn new() -> Self {
        Self {
            active: false,
            pid: 0,
            terminal: 0,
            descriptors: Vec::new(),
            cwd: String::new(),
            environment: Vec::new(),
            program_break: HEAP_START,
            heap_mapped_end: HEAP_INITIAL_END,
            heap_limit: HEAP_INITIAL_END,
            mmap_cursor: MMAP_START,
            heap_pages: Vec::new(),
            mmap_pages: Vec::new(),
            fpu_state: FpuState::new(),
        }
    }
}

struct ProcessKernelStack(Vec<u8>);

impl ProcessKernelStack {
    fn new() -> Result<Self, ElfError> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(PROCESS_KERNEL_STACK_SIZE)
            .map_err(|_| ElfError::OutOfMemory)?;
        bytes.resize(PROCESS_KERNEL_STACK_SIZE, 0);
        Ok(Self(bytes))
    }

    fn top(&self) -> u64 {
        ((self.0.as_ptr() as usize + self.0.len()) & !0xF) as u64
    }
}

struct ScheduledProcess {
    image: LoadedElf,
    state: UserProcess,
    pid: u64,
    _kernel_stack: Option<ProcessKernelStack>,
    kernel_stack_top: u64,
    frame_pointer: usize,
    exit_status: Option<u64>,
    parent_pid: Option<u64>,
    waiting_for: Option<ProcessWait>,
    wait_status_pointer: u64,
    reaped: bool,
}

struct UserScheduler {
    processes: Vec<ScheduledProcess>,
    current: usize,
    active: bool,
    next_pid: u64,
}

impl UserScheduler {
    const fn new() -> Self {
        Self {
            processes: Vec::new(),
            current: 0,
            active: false,
            next_pid: 1,
        }
    }
}

struct RestoreUserAddressSpace(u64);

impl Drop for RestoreUserAddressSpace {
    fn drop(&mut self) {
        let scheduler = USER_SCHEDULER.lock();
        if let Some(process) = scheduler
            .processes
            .iter()
            .find(|process| process.pid == self.0)
        {
            unsafe { process.image.address_space.activate() };
        } else {
            panic!("spawn parent disappeared before syscall returned");
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UserStat {
    size: u64,
    mode: u32,
    reserved: u32,
}

static USER_PROCESS: Mutex<UserProcess> = Mutex::new(UserProcess::new());
static USER_SCHEDULER: Mutex<UserScheduler> = Mutex::new(UserScheduler::new());
static NEXT_PIPE_ID: AtomicU64 = AtomicU64::new(1);

impl LoadedElf {
    /// Run one trusted program until it exits or faults, then release its pages.
    pub fn run(self) -> Result<u64, ElfError> {
        self.run_with_cwd("/")
    }

    /// Run one trusted program with a process-local current working directory.
    pub fn run_with_cwd(self, cwd: &str) -> Result<u64, ElfError> {
        self.run_with_cwd_on_terminal(cwd, crate::terminal::active_terminal())
    }

    pub fn run_with_cwd_on_terminal(self, cwd: &str, terminal: usize) -> Result<u64, ElfError> {
        let mut images = Vec::new();
        if images.try_reserve_exact(1).is_err() {
            self.unload()?;
            return Err(ElfError::OutOfMemory);
        }
        images.push(self);
        let statuses = run_loaded_processes(images, cwd, terminal)?;
        statuses
            .first()
            .copied()
            .ok_or(ElfError::SchedulerInvariant)
    }

    /// Release the mappings without running the program.
    pub fn unload(self) -> Result<(), ElfError> {
        self.unload_with_extra_pages(&mut Vec::new(), &mut Vec::new())
    }

    fn unload_with_extra_pages(
        mut self,
        heap_pages: &mut Vec<u64>,
        mmap_pages: &mut Vec<u64>,
    ) -> Result<(), ElfError> {
        unsafe { self.address_space.activate() };
        let page_cleanup = release_pages(&mut self.mapped_pages);
        let heap_cleanup = release_pages(heap_pages);
        let mmap_cleanup = release_pages(mmap_pages);
        let table_cleanup = self.address_space.destroy();
        page_cleanup?;
        heap_cleanup?;
        mmap_cleanup?;
        table_cleanup?;
        Ok(())
    }
}

pub fn load(bytes: &[u8]) -> Result<LoadedElf, ElfError> {
    load_with_args(bytes, &[])
}

pub fn load_with_args(bytes: &[u8], arguments: &[String]) -> Result<LoadedElf, ElfError> {
    load_with_args_and_environment(bytes, arguments, &[])
}

pub fn load_with_args_and_environment(
    bytes: &[u8],
    arguments: &[String],
    environment: &[String],
) -> Result<LoadedElf, ElfError> {
    let process_environment = try_clone_strings(environment)?;
    let (entry, segments) = parse(bytes)?;
    let heap_end = HEAP_INITIAL_END;
    let stack_start = STACK_TOP
        .checked_sub(STACK_PAGES * PAGE_SIZE)
        .ok_or(ElfError::AddressRange)?;
    let guard_start = stack_start
        .checked_sub(PAGE_SIZE)
        .ok_or(ElfError::AddressRange)?;

    let mut total_pages = STACK_PAGES + HEAP_PAGES;
    let mut heap_limit = MMAP_START;
    let mut mmap_cursor = MMAP_START;
    for (index, segment) in segments.iter().enumerate() {
        let start = page_start(segment.virtual_address);
        let end = page_end(segment.virtual_address, segment.memory_size)?;
        mmap_cursor = mmap_cursor.max(end);
        if (start < STACK_TOP && guard_start < end) || (start < heap_end && HEAP_START < end) {
            return Err(ElfError::AddressRange);
        }
        if start >= heap_end && start < heap_limit {
            heap_limit = start;
        }
        total_pages = total_pages
            .checked_add((end - start) / PAGE_SIZE)
            .ok_or(ElfError::TooLarge)?;
        for other in segments.iter().take(index) {
            let other_start = page_start(other.virtual_address);
            let other_end = page_end(other.virtual_address, other.memory_size)?;
            if start < other_end && other_start < end {
                return Err(ElfError::OverlappingSegments);
            }
        }
    }
    if !segments.iter().any(|segment| {
        segment.flags & PF_X != 0
            && entry >= segment.virtual_address
            && entry < segment.virtual_address + segment.memory_size
    }) {
        return Err(ElfError::InvalidEntryPoint);
    }

    unsafe { paging::enable_nxe()? };

    let address_space = paging::AddressSpace::new()?;
    unsafe { address_space.activate() };

    let mut mapped_pages = Vec::new();
    if mapped_pages
        .try_reserve_exact(usize::try_from(total_pages).map_err(|_| ElfError::TooLarge)?)
        .is_err()
    {
        address_space.destroy()?;
        return Err(ElfError::OutOfMemory);
    }

    let load_result = (|| {
        for segment in &segments {
            let start = page_start(segment.virtual_address);
            let end = page_end(segment.virtual_address, segment.memory_size)?;
            let mut page = start;
            while page < end {
                let mut flags = paging::flags::USER;
                if segment.flags & PF_W != 0 {
                    flags |= paging::flags::WRITABLE;
                }
                if segment.flags & PF_X == 0 {
                    flags |= paging::flags::NO_EXECUTE;
                }
                map_zeroed_page(page, flags, &mut mapped_pages)?;
                page += PAGE_SIZE;
            }
            if segment.file_size != 0 {
                let source = &bytes[segment.file_offset..segment.file_offset + segment.file_size];
                copy_to_user_pages(segment.virtual_address, source)?;
            }
        }

        for page_index in 0..STACK_PAGES {
            map_zeroed_page(
                stack_start + page_index * PAGE_SIZE,
                paging::flags::USER | paging::flags::WRITABLE | paging::flags::NO_EXECUTE,
                &mut mapped_pages,
            )?;
        }
        for page_index in 0..HEAP_PAGES {
            map_zeroed_page(
                HEAP_START + page_index * PAGE_SIZE,
                paging::flags::USER | paging::flags::WRITABLE | paging::flags::NO_EXECUTE,
                &mut mapped_pages,
            )?;
        }

        let (user_stack, initial_stack) =
            build_initial_stack(stack_start, STACK_PAGES * PAGE_SIZE, arguments, environment)?;
        copy_to_user_pages(user_stack, &initial_stack)?;
        Ok::<u64, ElfError>(user_stack)
    })();

    match load_result {
        Ok(user_stack) => {
            unsafe { address_space.activate_kernel() };
            Ok(LoadedElf {
                entry,
                user_stack,
                heap_limit,
                mmap_cursor,
                environment: process_environment,
                mapped_pages,
                address_space,
            })
        }
        Err(error) => {
            let page_cleanup = release_pages(&mut mapped_pages);
            let table_cleanup = address_space.destroy();
            page_cleanup?;
            table_cleanup?;
            Err(error)
        }
    }
}

/// Load and synchronously run an ELF file from the mounted VFS.
pub fn run_file(path: &str) -> Result<u64, ElfError> {
    run_file_from(path, "/")
}

/// Load and synchronously run an ELF file, inheriting the caller's working directory.
pub fn run_file_from(path: &str, cwd: &str) -> Result<u64, ElfError> {
    run_file_from_with_args(path, cwd, &[])
}

/// Load and run an ELF with the supplied `argv` vector and working directory.
pub fn run_file_from_with_args(
    path: &str,
    cwd: &str,
    arguments: &[String],
) -> Result<u64, ElfError> {
    run_file_from_with_args_on_terminal(path, cwd, arguments, crate::terminal::active_terminal())
}

pub fn run_file_from_with_args_on_terminal(
    path: &str,
    cwd: &str,
    arguments: &[String],
    terminal: usize,
) -> Result<u64, ElfError> {
    run_file_from_with_args_and_environment_on_terminal(path, cwd, arguments, &[], terminal)
}

pub fn run_file_from_with_args_and_environment_on_terminal(
    path: &str,
    cwd: &str,
    arguments: &[String],
    environment: &[String],
    terminal: usize,
) -> Result<u64, ElfError> {
    let bytes = read_executable_file(path)?;
    load_with_args_and_environment(&bytes, arguments, environment)?
        .run_with_cwd_on_terminal(cwd, terminal)
}

/// Run several ELF files concurrently under the user-process scheduler.
pub fn run_files_parallel_from(paths: &[String], cwd: &str) -> Result<Vec<u64>, ElfError> {
    run_files_parallel_from_on_terminal(paths, cwd, crate::terminal::active_terminal())
}

pub fn run_files_parallel_from_on_terminal(
    paths: &[String],
    cwd: &str,
    terminal: usize,
) -> Result<Vec<u64>, ElfError> {
    run_files_parallel_from_with_environment_on_terminal(paths, cwd, &[], terminal)
}

pub fn run_files_parallel_from_with_environment_on_terminal(
    paths: &[String],
    cwd: &str,
    environment: &[String],
    terminal: usize,
) -> Result<Vec<u64>, ElfError> {
    if paths.len() < 2 {
        return Err(ElfError::InvalidArguments);
    }

    let mut images = Vec::new();
    images
        .try_reserve_exact(paths.len())
        .map_err(|_| ElfError::OutOfMemory)?;
    for path in paths {
        let bytes = match read_executable_file(path) {
            Ok(bytes) => bytes,
            Err(error) => return fail_after_unload(images, error),
        };
        match load_with_args_and_environment(&bytes, core::slice::from_ref(path), environment) {
            Ok(image) => images.push(image),
            Err(error) => return fail_after_unload(images, error),
        }
    }

    run_loaded_processes(images, cwd, terminal)
}

fn read_executable_file(path: &str) -> Result<Vec<u8>, ElfError> {
    fs::with_vfs(|vfs| {
        let metadata = vfs.metadata(path)?;
        if metadata.permissions & 0o111 == 0 {
            return Err(ElfError::PermissionDenied);
        }
        if metadata.len == 0 {
            return Err(ElfError::InvalidHeader);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(metadata.len)
            .map_err(|_| ElfError::OutOfMemory)?;
        bytes.resize(metadata.len, 0);
        let read = vfs.read(path, 0, &mut bytes)?;
        if read != bytes.len() {
            return Err(ElfError::OutOfBounds);
        }
        Ok::<_, ElfError>(bytes)
    })?
}

fn run_loaded_processes(
    images: Vec<LoadedElf>,
    cwd: &str,
    terminal: usize,
) -> Result<Vec<u64>, ElfError> {
    if images.is_empty() {
        return Err(ElfError::InvalidArguments);
    }
    if images.len() > MAX_SCHEDULED_PROCESSES {
        return fail_after_unload(images, ElfError::InvalidArguments);
    }
    let scheduler_busy = {
        let scheduler = USER_SCHEDULER.lock();
        scheduler.active || !scheduler.processes.is_empty()
    };
    if scheduler_busy || USER_PROCESS.lock().active {
        return fail_after_unload(images, ElfError::ProcessAlreadyRunning);
    }

    let mut states = Vec::new();
    if states.try_reserve_exact(images.len()).is_err() {
        return fail_after_unload(images, ElfError::OutOfMemory);
    }
    for (index, image) in images.iter().enumerate() {
        let pid = index as u64 + 1;
        let process_environment = match try_clone_strings(&image.environment) {
            Ok(environment) => environment,
            Err(error) => return fail_after_unload(images, error),
        };
        match new_user_process(
            image.heap_limit,
            image.mmap_cursor,
            cwd,
            pid,
            terminal,
            process_environment,
        ) {
            Ok(state) => states.push(state),
            Err(error) => return fail_after_unload(images, error),
        }
    }

    let mut stacks = Vec::new();
    if stacks.try_reserve_exact(images.len()).is_err() {
        return fail_after_unload(images, ElfError::OutOfMemory);
    }
    stacks.push(None);
    for _ in 1..images.len() {
        match ProcessKernelStack::new() {
            Ok(stack) => stacks.push(Some(stack)),
            Err(error) => return fail_after_unload(images, error),
        }
    }

    let mut processes = Vec::new();
    if processes.try_reserve_exact(images.len()).is_err() {
        return fail_after_unload(images, ElfError::OutOfMemory);
    }
    let next_pid = images.len() as u64 + 1;
    for (index, ((image, state), kernel_stack)) in
        images.into_iter().zip(states).zip(stacks).enumerate()
    {
        let kernel_stack_top = kernel_stack
            .as_ref()
            .map_or_else(gdt::user_kernel_stack_top, ProcessKernelStack::top);
        let frame_pointer = kernel_stack
            .as_ref()
            .map_or(0, |stack| initialize_user_frame(&image, stack.top()));
        processes.push(ScheduledProcess {
            image,
            state,
            pid: index as u64 + 1,
            _kernel_stack: kernel_stack,
            kernel_stack_top,
            frame_pointer,
            exit_status: None,
            parent_pid: None,
            waiting_for: None,
            wait_status_pointer: 0,
            reaped: false,
        });
    }

    {
        let mut scheduler = USER_SCHEDULER.lock();
        scheduler.processes = processes;
        scheduler.current = 0;
        scheduler.active = true;
        scheduler.next_pid = next_pid;
        let mut active_process = USER_PROCESS.lock();
        core::mem::swap(&mut *active_process, &mut scheduler.processes[0].state);
        let first = &scheduler.processes[0].image;
        unsafe {
            first.address_space.activate();
        }
    }

    let (entry, user_stack) = {
        let scheduler = USER_SCHEDULER.lock();
        let first = &scheduler.processes[0].image;
        (first.entry, first.user_stack)
    };
    restore_current_user_fpu();
    let _last_status = unsafe { gdt::enter_user_mode(entry, user_stack) };
    restore_current_user_fpu();

    let mut scheduler = USER_SCHEDULER.lock();
    if let Some(first) = scheduler.processes.first() {
        unsafe {
            first.image.address_space.activate_kernel();
        }
    }
    let all_exited = scheduler
        .processes
        .iter()
        .all(|process| process.exit_status.is_some());
    scheduler.active = false;
    let processes = core::mem::take(&mut scheduler.processes);
    drop(scheduler);

    let mut statuses = Vec::new();
    let status_reserve_failed = statuses.try_reserve_exact(processes.len()).is_err();
    let mut missing_status = false;
    let mut cleanup_error = None;
    for mut process in processes {
        if let Some(status) = process.exit_status {
            if !status_reserve_failed {
                statuses.push(status);
            }
        } else {
            missing_status = true;
        }
        if let Err(error) = process
            .image
            .unload_with_extra_pages(&mut process.state.heap_pages, &mut process.state.mmap_pages)
            && cleanup_error.is_none()
        {
            cleanup_error = Some(error);
        }
    }
    if let Some(error) = cleanup_error {
        return Err(error);
    }
    if status_reserve_failed {
        return Err(ElfError::OutOfMemory);
    }
    if !all_exited || missing_status {
        return Err(ElfError::SchedulerInvariant);
    }
    Ok(statuses)
}

fn fail_after_unload<T>(images: Vec<LoadedElf>, error: ElfError) -> Result<T, ElfError> {
    unload_images(images)?;
    Err(error)
}

fn unload_images(images: Vec<LoadedElf>) -> Result<(), ElfError> {
    let mut first_error = None;
    for image in images {
        if let Err(error) = image.unload()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn initialize_user_frame(image: &LoadedElf, stack_top: u64) -> usize {
    let frame_pointer = (stack_top as usize - core::mem::size_of::<InterruptFrame>()) & !0xF;
    let frame = InterruptFrame {
        r15: 0,
        r14: 0,
        r13: 0,
        r12: 0,
        r11: 0,
        r10: 0,
        r9: 0,
        r8: 0,
        rbp: 0,
        rdi: 0,
        rsi: 0,
        rdx: 0,
        rcx: 0,
        rbx: 0,
        rax: 0,
        vector: u64::from(interrupt::TIMER_VECTOR),
        error_code: 0,
        stack_frame: InterruptStackFrame {
            instruction_pointer: image.entry,
            code_segment: u64::from(gdt::USER_CODE_SELECTOR),
            cpu_flags: 0x202,
            stack_pointer: image.user_stack,
            stack_segment: u64::from(gdt::USER_DATA_SELECTOR),
        },
    };
    unsafe {
        (frame_pointer as *mut InterruptFrame).write(frame);
    }
    frame_pointer
}

fn build_initial_stack(
    stack_start: u64,
    stack_size: u64,
    arguments: &[String],
    environment: &[String],
) -> Result<(u64, Vec<u8>), ElfError> {
    if arguments.len() > MAX_ARGUMENTS || environment.len() > MAX_ENVIRONMENT_VARIABLES {
        return Err(ElfError::ArgumentsTooLarge);
    }
    let argument_bytes = arguments.iter().try_fold(0usize, |total, argument| {
        if argument.as_bytes().contains(&0) {
            return Err(ElfError::InvalidArguments);
        }
        let argument_size = argument
            .len()
            .checked_add(1)
            .ok_or(ElfError::ArgumentsTooLarge)?;
        total
            .checked_add(argument_size)
            .ok_or(ElfError::ArgumentsTooLarge)
    })?;
    if argument_bytes > MAX_ARGUMENT_BYTES {
        return Err(ElfError::ArgumentsTooLarge);
    }
    let environment_bytes = environment.iter().try_fold(0usize, |total, variable| {
        let Some((name, _)) = variable.split_once('=') else {
            return Err(ElfError::InvalidArguments);
        };
        if !valid_environment_key(name) || variable.as_bytes().contains(&0) {
            return Err(ElfError::InvalidArguments);
        }
        total
            .checked_add(
                variable
                    .len()
                    .checked_add(1)
                    .ok_or(ElfError::ArgumentsTooLarge)?,
            )
            .ok_or(ElfError::ArgumentsTooLarge)
    })?;
    if environment_bytes > MAX_ENVIRONMENT_BYTES {
        return Err(ElfError::ArgumentsTooLarge);
    }
    let stack_size = usize::try_from(stack_size).map_err(|_| ElfError::ArgumentsTooLarge)?;
    let stack_size_u64 = u64::try_from(stack_size).map_err(|_| ElfError::ArgumentsTooLarge)?;
    let stack_end = stack_start
        .checked_add(stack_size_u64)
        .ok_or(ElfError::AddressRange)?;
    let mut stack = Vec::new();
    stack
        .try_reserve_exact(stack_size)
        .map_err(|_| ElfError::OutOfMemory)?;
    stack.resize(stack_size, 0);

    let mut cursor = stack_size;
    let mut pointers = Vec::new();
    pointers
        .try_reserve_exact(arguments.len())
        .map_err(|_| ElfError::OutOfMemory)?;
    for argument in arguments.iter().rev() {
        let size = argument
            .len()
            .checked_add(1)
            .ok_or(ElfError::ArgumentsTooLarge)?;
        cursor = cursor
            .checked_sub(size)
            .ok_or(ElfError::ArgumentsTooLarge)?;
        stack[cursor..cursor + argument.len()].copy_from_slice(argument.as_bytes());
        pointers.push(
            stack_start
                .checked_add(cursor as u64)
                .ok_or(ElfError::AddressRange)?,
        );
    }
    pointers.reverse();

    let mut environment_pointers = Vec::new();
    environment_pointers
        .try_reserve_exact(environment.len())
        .map_err(|_| ElfError::OutOfMemory)?;
    for variable in environment.iter().rev() {
        let size = variable
            .len()
            .checked_add(1)
            .ok_or(ElfError::ArgumentsTooLarge)?;
        cursor = cursor
            .checked_sub(size)
            .ok_or(ElfError::ArgumentsTooLarge)?;
        stack[cursor..cursor + variable.len()].copy_from_slice(variable.as_bytes());
        environment_pointers.push(
            stack_start
                .checked_add(cursor as u64)
                .ok_or(ElfError::AddressRange)?,
        );
    }
    environment_pointers.reverse();

    let table_words = arguments
        .len()
        .checked_add(environment.len())
        .ok_or(ElfError::ArgumentsTooLarge)?
        .checked_add(3)
        .ok_or(ElfError::ArgumentsTooLarge)?;
    let table_bytes = table_words
        .checked_mul(core::mem::size_of::<u64>())
        .ok_or(ElfError::ArgumentsTooLarge)?;
    cursor = cursor
        .checked_sub(table_bytes)
        .ok_or(ElfError::ArgumentsTooLarge)?
        & !0xF;
    let stack_pointer = stack_start
        .checked_add(cursor as u64)
        .ok_or(ElfError::AddressRange)?;
    if stack_pointer >= stack_end {
        return Err(ElfError::ArgumentsTooLarge);
    }

    let write_word = |stack: &mut [u8], offset: usize, value: u64| {
        stack[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    };
    write_word(&mut stack, cursor, arguments.len() as u64);
    for (index, pointer) in pointers.iter().enumerate() {
        write_word(&mut stack, cursor + 8 * (index + 1), *pointer);
    }
    let environment_start = arguments.len() + 2;
    for (index, pointer) in environment_pointers.iter().enumerate() {
        write_word(
            &mut stack,
            cursor + 8 * (environment_start + index),
            *pointer,
        );
    }
    write_word(
        &mut stack,
        cursor + 8 * (arguments.len() + environment.len() + 2),
        0,
    );
    let mut initial_stack = Vec::new();
    initial_stack
        .try_reserve_exact(stack_size - cursor)
        .map_err(|_| ElfError::OutOfMemory)?;
    initial_stack.extend_from_slice(&stack[cursor..]);
    Ok((stack_pointer, initial_stack))
}

pub fn handle_syscall(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    save_current_user_fpu();
    let next_frame = handle_syscall_inner(frame);
    restore_current_user_fpu();
    next_frame
}

fn handle_syscall_inner(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    match frame.rax {
        SYSCALL_EXIT => terminate_user_inner(frame, frame.rdi),
        SYSCALL_WAITPID => syscall_waitpid(frame),
        SYSCALL_PRINTF => {
            frame.rax = match syscall_printf(frame) {
                Ok(value) => value,
                Err(errno) => (-(errno as i64)) as u64,
            };
            frame_pointer
        }
        number => {
            let result = dispatch_syscall(number, frame.rdi, frame.rsi, frame.rdx, frame.r10);
            if result == Err(EAGAIN) {
                let pipe_wait = match number {
                    SYSCALL_READ => pipe_wait_target(frame.rdi, true),
                    SYSCALL_WRITE => pipe_wait_target(frame.rdi, false),
                    _ => None,
                };
                if let Some((_, wait_reason)) = pipe_wait {
                    return block_current_on_pipe(frame, wait_reason);
                }
            }
            frame.rax = match result {
                Ok(value) => value,
                Err(errno) => (-(errno as i64)) as u64,
            };
            frame_pointer
        }
    }
}

pub fn handle_timer(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let from_user = frame.stack_frame.code_segment & 3 == 3;
    if from_user {
        save_current_user_fpu();
    }
    let next_frame = handle_timer_inner(frame);
    if from_user {
        restore_current_user_fpu();
    }
    next_frame
}

fn handle_timer_inner(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    if frame.stack_frame.code_segment & 3 != 3 {
        return frame_pointer;
    }

    let mut scheduler = USER_SCHEDULER.lock();
    if !scheduler.active || scheduler.processes.is_empty() {
        return frame_pointer;
    }

    let current = scheduler.current;
    if current >= scheduler.processes.len() {
        return frame_pointer;
    }
    scheduler.processes[current].frame_pointer = frame_pointer as usize;
    let Some(next) = next_runnable(scheduler.processes.len(), current, |index| {
        is_process_runnable(&scheduler.processes[index])
    }) else {
        return frame_pointer;
    };
    if next == current {
        return frame_pointer;
    }

    let mut active_process = USER_PROCESS.lock();
    core::mem::swap(
        &mut *active_process,
        &mut scheduler.processes[current].state,
    );
    core::mem::swap(&mut *active_process, &mut scheduler.processes[next].state);
    scheduler.current = next;
    let next_process = &scheduler.processes[next];
    let next_frame = next_process.frame_pointer as *mut InterruptFrame;
    unsafe {
        gdt::set_ring0_stack_top(next_process.kernel_stack_top);
        next_process.image.address_space.activate();
    }
    next_frame
}

fn dispatch_syscall(number: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64) -> Result<u64, i32> {
    match number {
        SYSCALL_WRITE => syscall_write(arg1, arg2, arg3),
        SYSCALL_READ => syscall_read(arg1, arg2, arg3),
        SYSCALL_OPEN => syscall_open(arg1, arg2, arg3),
        SYSCALL_CLOSE => syscall_close(arg1),
        SYSCALL_LSEEK => syscall_lseek(arg1, arg2 as i64, arg3),
        SYSCALL_STAT => syscall_stat(arg1, arg2),
        SYSCALL_MKDIR => syscall_mkdir(arg1),
        SYSCALL_UNLINK => syscall_unlink(arg1),
        SYSCALL_FTRUNCATE => syscall_ftruncate(arg1, arg2),
        SYSCALL_BRK => syscall_brk(arg1),
        SYSCALL_CHDIR => syscall_chdir(arg1),
        SYSCALL_GETCWD => syscall_getcwd(arg1, arg2),
        SYSCALL_CHMOD => syscall_chmod(arg1, arg2),
        SYSCALL_GETPID => {
            let process = USER_PROCESS.lock();
            if process.active {
                Ok(process.pid)
            } else {
                Err(EINTR)
            }
        }
        SYSCALL_PUTPIXEL => syscall_putpixel(arg1, arg2, arg3),
        SYSCALL_MMAP => syscall_mmap(arg1, arg2, arg3),
        SYSCALL_MUNMAP => syscall_munmap(arg1, arg2),
        SYSCALL_MMAP_FILE => syscall_mmap_file(arg1, arg2, arg3),
        SYSCALL_SPAWN => syscall_spawn(arg1, arg2, arg3),
        SYSCALL_SPAWN_ENV => syscall_spawn_with_environment(arg1, arg2, arg3, arg4),
        SYSCALL_PIPE => syscall_pipe(arg1),
        SYSCALL_DUP2 => syscall_dup2(arg1, arg2),
        SYSCALL_SETENVIRON => syscall_set_environ(arg1),
        SYSCALL_OPENDIR => syscall_opendir(arg1),
        SYSCALL_READDIR => syscall_readdir(arg1, arg2),
        SYSCALL_ISATTY => {
            let fd = usize::try_from(arg1).map_err(|_| EBADF)?;
            let process = USER_PROCESS.lock();
            if !process.active {
                return Err(EINTR);
            }
            match process.descriptors.get(fd).and_then(Option::as_ref) {
                Some(
                    UserDescriptor::ConsoleInput
                    | UserDescriptor::ConsoleOutput
                    | UserDescriptor::ConsoleError,
                ) => Ok(1),
                Some(UserDescriptor::File(file)) if fs::is_tty_path(&file.path) => Ok(1),
                Some(UserDescriptor::File(_)) => Err(ENOTTY),
                Some(UserDescriptor::Directory(_)) => Err(ENOTTY),
                Some(UserDescriptor::Pipe(_)) => Err(ENOTTY),
                None => Err(EBADF),
            }
        }
        _ => Err(ENOSYS),
    }
}

fn syscall_spawn(path_address: u64, argv_address: u64, argc: u64) -> Result<u64, i32> {
    syscall_spawn_with_environment(path_address, argv_address, argc, 0)
}

fn syscall_spawn_with_environment(
    path_address: u64,
    argv_address: u64,
    argc: u64,
    environment_address: u64,
) -> Result<u64, i32> {
    let path = resolve_process_path(&copy_user_path(path_address)?)?;
    let arguments = copy_user_arguments(argv_address, argc)?;
    let provided_environment = if environment_address == 0 {
        None
    } else {
        Some(copy_user_environment(environment_address)?)
    };
    let (cwd, terminal, parent_pid, inherited_descriptors, environment) = {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        let environment = match provided_environment {
            Some(environment) => environment,
            None => try_clone_strings(&process.environment).map_err(|_| ENOMEM)?,
        };
        let mut descriptors = Vec::new();
        descriptors
            .try_reserve_exact(process.descriptors.len())
            .map_err(|_| ENOMEM)?;
        descriptors.extend(process.descriptors.iter().cloned());
        (
            process.cwd.clone(),
            process.terminal,
            process.pid,
            descriptors,
            environment,
        )
    };
    let _restore_address_space = RestoreUserAddressSpace(parent_pid);

    let pid = {
        let mut scheduler = USER_SCHEDULER.lock();
        if !scheduler.active
            || scheduler
                .processes
                .get(scheduler.current)
                .is_none_or(|process| process.pid != parent_pid)
        {
            return Err(EINTR);
        }
        reap_completed_processes(&mut scheduler)?;
        if scheduler.processes.len() >= MAX_SCHEDULED_PROCESSES {
            return Err(ENOMEM);
        }
        scheduler.processes.try_reserve(1).map_err(|_| ENOMEM)?;
        let pid = scheduler.next_pid;
        scheduler.next_pid = pid.checked_add(1).ok_or(ENOMEM)?;
        pid
    };

    let bytes = read_executable_file(&path).map_err(errno_from_elf)?;
    {
        let scheduler = USER_SCHEDULER.lock();
        let parent = scheduler
            .processes
            .iter()
            .find(|process| process.pid == parent_pid)
            .ok_or(EINTR)?;
        unsafe { parent.image.address_space.activate_kernel() };
    }
    let image =
        load_with_args_and_environment(&bytes, &arguments, &environment).map_err(errno_from_elf)?;
    let mut state = match new_user_process(
        image.heap_limit,
        image.mmap_cursor,
        &cwd,
        pid,
        terminal,
        environment,
    ) {
        Ok(state) => state,
        Err(error) => {
            return Err(cleanup_failed_spawn(image, errno_from_elf(error)));
        }
    };
    state.descriptors = inherited_descriptors;
    let kernel_stack = match ProcessKernelStack::new() {
        Ok(stack) => stack,
        Err(error) => {
            return Err(cleanup_failed_spawn(image, errno_from_elf(error)));
        }
    };
    let kernel_stack_top = kernel_stack.top();
    let frame_pointer = initialize_user_frame(&image, kernel_stack_top);
    let mut scheduler = USER_SCHEDULER.lock();
    if !scheduler.active || scheduler.processes.len() >= MAX_SCHEDULED_PROCESSES {
        drop(scheduler);
        return Err(cleanup_failed_spawn(image, ENOMEM));
    }
    scheduler.processes.push(ScheduledProcess {
        image,
        state,
        pid,
        _kernel_stack: Some(kernel_stack),
        kernel_stack_top,
        frame_pointer,
        exit_status: None,
        parent_pid: Some(parent_pid),
        waiting_for: None,
        wait_status_pointer: 0,
        reaped: false,
    });
    Ok(pid)
}

fn syscall_set_environ(address: u64) -> Result<u64, i32> {
    let environment = copy_user_environment(address)?;
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    process.environment = environment;
    Ok(0)
}

fn cleanup_failed_spawn(image: LoadedElf, original_error: i32) -> i32 {
    match image.unload() {
        Ok(()) => original_error,
        Err(_) => EIO,
    }
}

fn reap_completed_processes(scheduler: &mut UserScheduler) -> Result<(), i32> {
    let mut index = 0;
    while index < scheduler.processes.len() {
        let process = &scheduler.processes[index];
        if index == scheduler.current || !process.reaped || process.exit_status.is_none() {
            index += 1;
            continue;
        }

        let mut process = scheduler.processes.remove(index);
        if index < scheduler.current {
            scheduler.current -= 1;
        }
        process
            .image
            .unload_with_extra_pages(&mut process.state.heap_pages, &mut process.state.mmap_pages)
            .map_err(|_| EIO)?;
    }
    Ok(())
}

fn errno_from_elf(error: ElfError) -> i32 {
    match error {
        ElfError::FileSystem(error) => errno_from_fs(error),
        ElfError::OutOfMemory | ElfError::Paging(PagingError::OutOfFrames) => ENOMEM,
        ElfError::PermissionDenied => EACCES,
        ElfError::InvalidArguments | ElfError::ArgumentsTooLarge => EINVAL,
        ElfError::ProcessAlreadyRunning => EINTR,
        _ => ENOEXEC,
    }
}

fn syscall_waitpid(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    let wait_target = frame.rdi;
    let status_pointer = frame.rsi;
    if wait_target == 0 {
        frame.rax = (-(EINVAL as i64)) as u64;
        return frame_pointer;
    }
    if status_pointer != 0
        && paging::validate_user_range(status_pointer, core::mem::size_of::<u64>(), true).is_err()
    {
        frame.rax = (-(EFAULT as i64)) as u64;
        return frame_pointer;
    }

    let mut scheduler = USER_SCHEDULER.lock();
    let current = scheduler.current;
    let parent_pid = {
        let process = USER_PROCESS.lock();
        if !scheduler.active || !process.active || current >= scheduler.processes.len() {
            frame.rax = (-(EINTR as i64)) as u64;
            return frame_pointer;
        }
        process.pid
    };
    let Some(child) = scheduler.processes.iter().position(|process| {
        process.parent_pid == Some(parent_pid)
            && !process.reaped
            && wait_target_matches(wait_target, process.pid)
    }) else {
        frame.rax = (-(ECHILD as i64)) as u64;
        return frame_pointer;
    };
    let child_pid = scheduler.processes[child].pid;

    if let Some(status) = scheduler.processes[child].exit_status {
        if status_pointer != 0
            && let Err(error) = write_user_u64(status_pointer, status)
        {
            frame.rax = (-(error as i64)) as u64;
            return frame_pointer;
        }
        scheduler.processes[child].reaped = true;
        frame.rax = child_pid;
        return frame_pointer;
    }

    scheduler.processes[current].frame_pointer = frame_pointer as usize;
    scheduler.processes[current].waiting_for = Some(ProcessWait::Child(wait_target));
    scheduler.processes[current].wait_status_pointer = status_pointer;
    let mut active_process = USER_PROCESS.lock();
    core::mem::swap(
        &mut *active_process,
        &mut scheduler.processes[current].state,
    );
    let Some(next) = next_runnable(scheduler.processes.len(), current, |index| {
        is_process_runnable(&scheduler.processes[index])
    }) else {
        core::mem::swap(
            &mut *active_process,
            &mut scheduler.processes[current].state,
        );
        scheduler.processes[current].waiting_for = None;
        scheduler.processes[current].wait_status_pointer = 0;
        frame.rax = (-(ECHILD as i64)) as u64;
        return frame_pointer;
    };
    core::mem::swap(&mut *active_process, &mut scheduler.processes[next].state);
    scheduler.current = next;
    let next_process = &scheduler.processes[next];
    let next_frame = next_process.frame_pointer as *mut InterruptFrame;
    unsafe {
        gdt::set_ring0_stack_top(next_process.kernel_stack_top);
        next_process.image.address_space.activate();
    }
    next_frame
}

fn wait_target_matches(target: u64, child_pid: u64) -> bool {
    target == WAIT_ANY_PID || target == child_pid
}

fn write_user_u64(address: u64, value: u64) -> Result<(), i32> {
    paging::validate_user_range(address, core::mem::size_of::<u64>(), true).map_err(|_| EFAULT)?;
    unsafe { (address as *mut u64).write_unaligned(value) };
    Ok(())
}

fn pipe_wait_target(fd: u64, reading: bool) -> Option<(u64, ProcessWait)> {
    let fd = usize::try_from(fd).ok()?;
    let process = USER_PROCESS.lock();
    if !process.active {
        return None;
    }
    let Some(UserDescriptor::Pipe(endpoint)) = process.descriptors.get(fd).and_then(Option::as_ref)
    else {
        return None;
    };
    let pipe_id = endpoint.pipe.lock().id;
    let wait = match (reading, endpoint.readable, endpoint.writable) {
        (true, true, _) => ProcessWait::PipeReadable(pipe_id),
        (false, _, true) => ProcessWait::PipeWritable(pipe_id),
        _ => return None,
    };
    Some((pipe_id, wait))
}

fn block_current_on_pipe(frame: &mut InterruptFrame, wait: ProcessWait) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    let Some(retry_address) = frame.stack_frame.instruction_pointer.checked_sub(2) else {
        frame.rax = (-(EINVAL as i64)) as u64;
        return frame_pointer;
    };
    let mut scheduler = USER_SCHEDULER.lock();
    let current = scheduler.current;
    {
        let process = USER_PROCESS.lock();
        if !scheduler.active || !process.active || current >= scheduler.processes.len() {
            frame.rax = (-(EINTR as i64)) as u64;
            return frame_pointer;
        }
    }

    scheduler.processes[current].frame_pointer = frame_pointer as usize;
    scheduler.processes[current].waiting_for = Some(wait);
    // Retry the int 0x80 instruction when the pipe becomes ready.
    frame.stack_frame.instruction_pointer = retry_address;
    let mut active_process = USER_PROCESS.lock();
    core::mem::swap(
        &mut *active_process,
        &mut scheduler.processes[current].state,
    );
    let Some(next) = next_runnable(scheduler.processes.len(), current, |index| {
        is_process_runnable(&scheduler.processes[index])
    }) else {
        core::mem::swap(
            &mut *active_process,
            &mut scheduler.processes[current].state,
        );
        scheduler.processes[current].waiting_for = None;
        frame.stack_frame.instruction_pointer = retry_address + 2;
        frame.rax = (-(EDEADLK as i64)) as u64;
        return frame_pointer;
    };
    core::mem::swap(&mut *active_process, &mut scheduler.processes[next].state);
    scheduler.current = next;
    let next_process = &scheduler.processes[next];
    let next_frame = next_process.frame_pointer as *mut InterruptFrame;
    unsafe {
        gdt::set_ring0_stack_top(next_process.kernel_stack_top);
        next_process.image.address_space.activate();
    }
    next_frame
}

fn wake_pipe_waiters(pipe_id: u64, readers_ready: bool, writers_ready: bool) {
    let mut scheduler = USER_SCHEDULER.lock();
    wake_pipe_waiters_in_scheduler(&mut scheduler, pipe_id, readers_ready, writers_ready);
}

fn wake_pipe_waiters_in_scheduler(
    scheduler: &mut UserScheduler,
    pipe_id: u64,
    readers_ready: bool,
    writers_ready: bool,
) {
    for process in &mut scheduler.processes {
        let ready = match process.waiting_for {
            Some(ProcessWait::PipeReadable(waiting_id)) => readers_ready && waiting_id == pipe_id,
            Some(ProcessWait::PipeWritable(waiting_id)) => writers_ready && waiting_id == pipe_id,
            _ => false,
        };
        if ready {
            process.waiting_for = None;
        }
    }
}

fn new_user_process(
    heap_limit: u64,
    mmap_cursor: u64,
    cwd: &str,
    pid: u64,
    terminal: usize,
    environment: Vec<String>,
) -> Result<UserProcess, ElfError> {
    let cwd = fs::normalize_path(cwd)?;
    let metadata = fs::with_vfs(|vfs| vfs.metadata(&cwd))??;
    if metadata.node_type != NodeType::Directory {
        return Err(ElfError::FileSystem(FsError::NotDirectory));
    }

    let mut descriptors = Vec::new();
    descriptors
        .try_reserve_exact(3)
        .map_err(|_| ElfError::OutOfMemory)?;
    descriptors.extend([
        Some(UserDescriptor::ConsoleInput),
        Some(UserDescriptor::ConsoleOutput),
        Some(UserDescriptor::ConsoleError),
    ]);
    let mut process_cwd = String::new();
    process_cwd
        .try_reserve(cwd.len())
        .map_err(|_| ElfError::OutOfMemory)?;
    process_cwd.push_str(&cwd);
    Ok(UserProcess {
        active: true,
        pid,
        terminal,
        descriptors,
        cwd: process_cwd,
        environment,
        program_break: HEAP_START,
        heap_mapped_end: HEAP_INITIAL_END,
        heap_limit,
        mmap_cursor,
        heap_pages: Vec::new(),
        mmap_pages: Vec::new(),
        fpu_state: FpuState::new(),
    })
}

fn save_current_user_fpu() {
    let mut process = USER_PROCESS.lock();
    if process.active {
        unsafe { fpu::save(&mut process.fpu_state) };
    }
}

fn restore_current_user_fpu() {
    let process = USER_PROCESS.lock();
    unsafe {
        if process.active {
            fpu::restore(&process.fpu_state);
        } else {
            fpu::restore(&FpuState::new());
        }
    }
}

fn try_clone_strings(values: &[String]) -> Result<Vec<String>, ElfError> {
    let mut cloned = Vec::new();
    cloned
        .try_reserve_exact(values.len())
        .map_err(|_| ElfError::OutOfMemory)?;
    for value in values {
        let mut copy = String::new();
        copy.try_reserve(value.len())
            .map_err(|_| ElfError::OutOfMemory)?;
        copy.push_str(value);
        cloned.push(copy);
    }
    Ok(cloned)
}

pub(crate) fn valid_environment_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

fn create_pipe() -> Result<(PipeEnd, PipeEnd), i32> {
    let id = NEXT_PIPE_ID
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| ENOMEM)?;
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(PIPE_CAPACITY)
        .map_err(|_| ENOMEM)?;
    buffer.resize(PIPE_CAPACITY, 0);
    let pipe = Arc::new(Mutex::new(Pipe {
        id,
        buffer,
        head: 0,
        length: 0,
        readers: 1,
        writers: 1,
    }));
    Ok((
        PipeEnd {
            pipe: Arc::clone(&pipe),
            readable: true,
            writable: false,
        },
        PipeEnd {
            pipe,
            readable: false,
            writable: true,
        },
    ))
}

fn reserve_descriptor_slot(descriptors: &mut Vec<Option<UserDescriptor>>) -> Result<usize, i32> {
    if let Some(fd) = descriptors
        .iter()
        .enumerate()
        .find_map(|(index, descriptor)| descriptor.is_none().then_some(index))
    {
        return Ok(fd);
    }
    if descriptors.len() >= MAX_OPEN_FILES {
        return Err(EMFILE);
    }
    descriptors.try_reserve(1).map_err(|_| ENOMEM)?;
    descriptors.push(None);
    Ok(descriptors.len() - 1)
}

fn install_pipe_descriptors(
    descriptors: &mut Vec<Option<UserDescriptor>>,
    read_end: PipeEnd,
    write_end: PipeEnd,
) -> Result<(usize, usize), i32> {
    let read_fd = reserve_descriptor_slot(descriptors)?;
    descriptors[read_fd] = Some(UserDescriptor::Pipe(read_end));
    let write_fd = match reserve_descriptor_slot(descriptors) {
        Ok(fd) => fd,
        Err(error) => {
            descriptors[read_fd] = None;
            while descriptors.last().is_some_and(Option::is_none) {
                descriptors.pop();
            }
            return Err(error);
        }
    };
    descriptors[write_fd] = Some(UserDescriptor::Pipe(write_end));
    Ok((read_fd, write_fd))
}

fn syscall_write(fd: u64, address: u64, length: u64) -> Result<u64, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let length = usize::try_from(length).map_err(|_| EINVAL)?;
    if length > MAX_SYSCALL_IO {
        return Err(EINVAL);
    }
    if length == 0 {
        return Ok(0);
    }
    paging::validate_user_range(address, length, false).map_err(|_| EFAULT)?;
    let bytes = unsafe { core::slice::from_raw_parts(address as *const u8, length) };
    syscall_write_bytes(fd, bytes)
}

fn syscall_write_bytes(fd: usize, bytes: &[u8]) -> Result<u64, i32> {
    if bytes.len() > MAX_SYSCALL_IO {
        return Err(EINVAL);
    }
    if bytes.is_empty() {
        return Ok(0);
    }
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let terminal = process.terminal;
    let descriptor = process
        .descriptors
        .get_mut(fd)
        .and_then(Option::as_mut)
        .ok_or(EBADF)?;
    let mut pipe_id = None;
    let result = match descriptor {
        UserDescriptor::ConsoleOutput | UserDescriptor::ConsoleError => {
            let text = core::str::from_utf8(bytes).map_err(|_| EILSEQ)?;
            crate::terminal::terminal_print_to(terminal, text);
            Ok(bytes.len() as u64)
        }
        UserDescriptor::File(file) => {
            if !file.writable {
                return Err(EBADF);
            }
            if fs::is_tty_path(&file.path) {
                return fs::with_vfs(|vfs| vfs.write(&file.path, 0, bytes))
                    .map_err(errno_from_fs)?
                    .map(|count| count as u64)
                    .map_err(errno_from_fs);
            }
            let offset = if file.append {
                fs::with_vfs(|vfs| vfs.metadata(&file.path).map(|metadata| metadata.len))
                    .map_err(errno_from_fs)?
                    .map_err(errno_from_fs)?
            } else {
                file.offset
            };
            let count = fs::with_vfs(|vfs| vfs.write(&file.path, offset, bytes))
                .map_err(errno_from_fs)?
                .map_err(errno_from_fs)?;
            file.offset = offset.checked_add(count).ok_or(EINVAL)?;
            Ok(count as u64)
        }
        UserDescriptor::Pipe(endpoint) => {
            if !endpoint.writable {
                return Err(EBADF);
            }
            let mut pipe = endpoint.pipe.lock();
            let count = pipe.write(bytes)?;
            pipe_id = Some(pipe.id);
            Ok(count as u64)
        }
        UserDescriptor::ConsoleInput | UserDescriptor::Directory(_) => Err(EBADF),
    };
    drop(process);
    if let Some(pipe_id) = pipe_id {
        wake_pipe_waiters(pipe_id, true, false);
    }
    result
}

fn syscall_printf(frame: &InterruptFrame) -> Result<u64, i32> {
    let format = copy_user_string(frame.rdi, MAX_SYSCALL_IO)?;
    let output = format_printf(
        &format,
        |index| printf_argument(frame, index),
        |address, limit| copy_user_string(address, limit),
    )?;
    syscall_write_bytes(1, &output)
}

fn printf_argument(frame: &InterruptFrame, index: usize) -> Result<u64, i32> {
    match index {
        0 => Ok(frame.rsi),
        1 => Ok(frame.rdx),
        2 => Ok(frame.rcx),
        3 => Ok(frame.r8),
        4 => Ok(frame.r9),
        _ => {
            let stack_index = index - 5;
            let offset = stack_index
                .checked_mul(core::mem::size_of::<u64>())
                .and_then(|offset| offset.checked_add(core::mem::size_of::<u64>()))
                .and_then(|offset| u64::try_from(offset).ok())
                .ok_or(EFAULT)?;
            let address = frame
                .stack_frame
                .stack_pointer
                .checked_add(offset)
                .ok_or(EFAULT)?;
            paging::validate_user_range(address, core::mem::size_of::<u64>(), false)
                .map_err(|_| EFAULT)?;
            Ok(unsafe { (address as *const u64).read_unaligned() })
        }
    }
}

fn format_printf(
    format: &str,
    mut argument: impl FnMut(usize) -> Result<u64, i32>,
    mut copy_string: impl FnMut(u64, usize) -> Result<String, i32>,
) -> Result<Vec<u8>, i32> {
    let input = format.as_bytes();
    let mut output = Vec::new();
    output.try_reserve(input.len()).map_err(|_| ENOMEM)?;
    let mut cursor = 0;
    let mut argument_index = 0;

    while cursor < input.len() {
        if input[cursor] != b'%' {
            append_printf_bytes(&mut output, &input[cursor..cursor + 1])?;
            cursor += 1;
            continue;
        }
        cursor += 1;
        if input.get(cursor) == Some(&b'%') {
            append_printf_bytes(&mut output, b"%")?;
            cursor += 1;
            continue;
        }

        let mut left = false;
        let mut plus = false;
        let mut space = false;
        let mut alternate = false;
        let mut zero = false;
        loop {
            match input.get(cursor) {
                Some(b'-') => left = true,
                Some(b'+') => plus = true,
                Some(b' ') => space = true,
                Some(b'#') => alternate = true,
                Some(b'0') => zero = true,
                _ => break,
            }
            cursor += 1;
        }

        let width = if input.get(cursor) == Some(&b'*') {
            cursor += 1;
            let width = argument(argument_index)? as i32;
            argument_index += 1;
            if width < 0 {
                left = true;
                width.unsigned_abs() as usize
            } else {
                width as usize
            }
        } else {
            parse_printf_number(input, &mut cursor)?.unwrap_or(0)
        };
        if width > MAX_SYSCALL_IO {
            return Err(EOVERFLOW);
        }

        let precision = if input.get(cursor) == Some(&b'.') {
            cursor += 1;
            if input.get(cursor) == Some(&b'*') {
                cursor += 1;
                let precision = argument(argument_index)? as i32;
                argument_index += 1;
                if precision < 0 {
                    None
                } else {
                    let precision = precision as usize;
                    if precision > MAX_SYSCALL_IO {
                        return Err(EOVERFLOW);
                    }
                    Some(precision)
                }
            } else {
                Some(parse_printf_number(input, &mut cursor)?.unwrap_or(0))
            }
        } else {
            None
        };

        let mut length_modifier = 0;
        if input.get(cursor) == Some(&b'h') {
            cursor += 1;
            length_modifier = 1;
            if input.get(cursor) == Some(&b'h') {
                cursor += 1;
                length_modifier = 2;
            }
        } else if input.get(cursor) == Some(&b'l') {
            cursor += 1;
            length_modifier = 3;
            if input.get(cursor) == Some(&b'l') {
                cursor += 1;
                length_modifier = 4;
            }
        } else if matches!(input.get(cursor), Some(b'z' | b't' | b'j')) {
            cursor += 1;
            length_modifier = 3;
        }

        let conversion = *input.get(cursor).ok_or(EINVAL)?;
        cursor += 1;
        match conversion {
            b's' => {
                let text = copy_string(argument(argument_index)?, MAX_SYSCALL_IO)?;
                argument_index += 1;
                let bytes = text.as_bytes();
                let count = precision.map_or(bytes.len(), |limit| bytes.len().min(limit));
                let padding = width.saturating_sub(count);
                if !left {
                    append_printf_repeat(&mut output, b' ', padding)?;
                }
                append_printf_bytes(&mut output, &bytes[..count])?;
                if left {
                    append_printf_repeat(&mut output, b' ', padding)?;
                }
            }
            b'c' => {
                let character = argument(argument_index)? as u8;
                argument_index += 1;
                let padding = width.saturating_sub(1);
                if !left {
                    append_printf_repeat(&mut output, b' ', padding)?;
                }
                append_printf_bytes(&mut output, &[character])?;
                if left {
                    append_printf_repeat(&mut output, b' ', padding)?;
                }
            }
            b'd' | b'i' => {
                let raw = argument(argument_index)?;
                argument_index += 1;
                let value = match length_modifier {
                    1 => raw as i16 as i64,
                    2 => raw as i8 as i64,
                    3 | 4 => raw as i64,
                    _ => raw as u32 as i32 as i64,
                };
                append_printf_integer(
                    &mut output,
                    value.unsigned_abs(),
                    value < 0,
                    10,
                    false,
                    width,
                    precision,
                    left,
                    zero,
                    plus,
                    space,
                    false,
                    false,
                )?;
            }
            b'u' | b'o' | b'x' | b'X' | b'p' => {
                let raw = argument(argument_index)?;
                argument_index += 1;
                let value = if length_modifier >= 3 || conversion == b'p' {
                    raw
                } else {
                    match length_modifier {
                        1 => raw as u16 as u64,
                        2 => raw as u8 as u64,
                        _ => raw as u32 as u64,
                    }
                };
                let base = match conversion {
                    b'o' => 8,
                    b'x' | b'X' | b'p' => 16,
                    _ => 10,
                };
                append_printf_integer(
                    &mut output,
                    value,
                    false,
                    base,
                    conversion == b'X',
                    width,
                    precision,
                    left,
                    zero,
                    false,
                    false,
                    alternate || conversion == b'p',
                    conversion == b'p',
                )?;
            }
            _ => return Err(EINVAL),
        }
    }
    Ok(output)
}

fn parse_printf_number(input: &[u8], cursor: &mut usize) -> Result<Option<usize>, i32> {
    let start = *cursor;
    let mut value = 0usize;
    while let Some(byte) = input.get(*cursor).copied().filter(u8::is_ascii_digit) {
        let digit = byte - b'0';
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(digit as usize))
            .filter(|value| *value <= MAX_SYSCALL_IO)
            .ok_or(EOVERFLOW)?;
        *cursor += 1;
    }
    Ok((*cursor > start).then_some(value))
}

fn append_printf_integer(
    output: &mut Vec<u8>,
    mut value: u64,
    negative: bool,
    base: u64,
    uppercase: bool,
    width: usize,
    precision: Option<usize>,
    left: bool,
    zero: bool,
    plus: bool,
    space: bool,
    alternate: bool,
    pointer: bool,
) -> Result<(), i32> {
    let alphabet = if uppercase {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut digits = [0; 64];
    let mut digit_count = 0;
    if value != 0 || precision != Some(0) {
        loop {
            digits[digit_count] = alphabet[(value % base) as usize];
            digit_count += 1;
            value /= base;
            if value == 0 {
                break;
            }
        }
    }

    let mut leading_zeroes = precision.unwrap_or(0).saturating_sub(digit_count);
    if base == 8
        && alternate
        && (digit_count == 0 || digits[digit_count - 1] != b'0')
        && leading_zeroes == 0
    {
        leading_zeroes = 1;
    }
    let sign = if negative {
        Some(b'-')
    } else if plus {
        Some(b'+')
    } else if space {
        Some(b' ')
    } else {
        None
    };
    let prefix = if pointer
        || (alternate && base == 16 && digit_count != 0 && !(digit_count == 1 && digits[0] == b'0'))
    {
        Some(if uppercase { b"0X" } else { b"0x" }.as_slice())
    } else {
        None
    };
    let prefix_length = prefix.map_or(0, <[u8]>::len);
    let total = digit_count
        .checked_add(leading_zeroes)
        .and_then(|length| length.checked_add(prefix_length))
        .and_then(|length| length.checked_add(usize::from(sign.is_some())))
        .ok_or(EOVERFLOW)?;
    let mut padding = width.saturating_sub(total);
    if !left && (!zero || precision.is_some()) {
        append_printf_repeat(output, b' ', padding)?;
    }
    if let Some(sign) = sign {
        append_printf_bytes(output, &[sign])?;
    }
    if let Some(prefix) = prefix {
        append_printf_bytes(output, prefix)?;
    }
    if !left && zero && precision.is_none() {
        leading_zeroes = leading_zeroes.checked_add(padding).ok_or(EOVERFLOW)?;
        padding = 0;
    }
    append_printf_repeat(output, b'0', leading_zeroes)?;
    while digit_count != 0 {
        digit_count -= 1;
        append_printf_bytes(output, &digits[digit_count..digit_count + 1])?;
    }
    if left {
        append_printf_repeat(output, b' ', padding)?;
    }
    Ok(())
}

fn append_printf_bytes(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), i32> {
    let new_length = output
        .len()
        .checked_add(bytes.len())
        .filter(|length| *length <= MAX_SYSCALL_IO)
        .ok_or(EOVERFLOW)?;
    output.try_reserve(bytes.len()).map_err(|_| ENOMEM)?;
    output.extend_from_slice(bytes);
    debug_assert_eq!(output.len(), new_length);
    Ok(())
}

fn append_printf_repeat(output: &mut Vec<u8>, byte: u8, count: usize) -> Result<(), i32> {
    let new_length = output
        .len()
        .checked_add(count)
        .filter(|length| *length <= MAX_SYSCALL_IO)
        .ok_or(EOVERFLOW)?;
    output.try_reserve(count).map_err(|_| ENOMEM)?;
    output.resize(new_length, byte);
    Ok(())
}

fn syscall_read(fd: u64, address: u64, length: u64) -> Result<u64, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let length = usize::try_from(length).map_err(|_| EINVAL)?;
    if length > MAX_SYSCALL_IO {
        return Err(EINVAL);
    }
    if length == 0 {
        return Ok(0);
    }
    paging::validate_user_range(address, length, true).map_err(|_| EFAULT)?;
    let output = unsafe { core::slice::from_raw_parts_mut(address as *mut u8, length) };
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let terminal = process.terminal;
    let descriptor = process
        .descriptors
        .get_mut(fd)
        .and_then(Option::as_mut)
        .ok_or(EBADF)?;
    let mut pipe_id = None;
    let result = match descriptor {
        UserDescriptor::ConsoleInput => fs::read_tty(terminal, output)
            .map(|count| count as u64)
            .map_err(errno_from_fs),
        UserDescriptor::File(file) => {
            if !file.readable {
                return Err(EBADF);
            }
            let count = fs::with_vfs(|vfs| vfs.read(&file.path, file.offset, output))
                .map_err(errno_from_fs)?
                .map_err(errno_from_fs)?;
            if !fs::is_tty_path(&file.path) {
                file.offset = file.offset.checked_add(count).ok_or(EINVAL)?;
            }
            Ok(count as u64)
        }
        UserDescriptor::Pipe(endpoint) => {
            if !endpoint.readable {
                return Err(EBADF);
            }
            let mut pipe = endpoint.pipe.lock();
            let count = pipe.read(output)?;
            pipe_id = Some(pipe.id);
            Ok(count as u64)
        }
        UserDescriptor::ConsoleOutput | UserDescriptor::ConsoleError => Err(EBADF),
        UserDescriptor::Directory(_) => Err(EBADF),
    };
    drop(process);
    if let Some(pipe_id) = pipe_id {
        wake_pipe_waiters(pipe_id, false, true);
    }
    result
}

fn syscall_open(address: u64, flags: u64, mode: u64) -> Result<u64, i32> {
    const ALLOWED_FLAGS: u64 = O_ACCESS_MASK | O_CREAT | O_TRUNC | O_APPEND;
    if flags & !ALLOWED_FLAGS != 0 || flags & O_ACCESS_MASK == 3 {
        return Err(EINVAL);
    }
    let mode = if flags & O_CREAT != 0 {
        let mode = u16::try_from(mode).map_err(|_| EINVAL)?;
        if mode & !0o777 != 0 {
            return Err(EINVAL);
        }
        mode
    } else {
        0o644
    };
    let writable = flags & O_ACCESS_MASK != O_RDONLY;
    let readable = flags & O_ACCESS_MASK != O_WRONLY;
    if flags & O_TRUNC != 0 && !writable || flags & O_APPEND != 0 && !writable {
        return Err(EINVAL);
    }
    let mut path = resolve_process_path(&copy_user_path(address)?)?;
    let process_terminal = USER_PROCESS.lock().terminal;
    if matches!(
        path.as_str(),
        "/dev/tty" | "/dev/stdin" | "/dev/stdout" | "/dev/stderr" | "/dev/console"
    ) {
        path = alloc::format!("/dev/tty{process_terminal}");
    }
    if let Some((device_readable, device_writable)) = fs::device_access(&path)
        && (readable && !device_readable || writable && !device_writable)
    {
        return Err(EACCES);
    }
    let fd = {
        let mut process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        if let Some(fd) = process
            .descriptors
            .iter()
            .enumerate()
            .find_map(|(index, descriptor)| descriptor.is_none().then_some(index))
        {
            fd
        } else {
            if process.descriptors.len() >= MAX_OPEN_FILES {
                return Err(EMFILE);
            }
            process.descriptors.try_reserve(1).map_err(|_| ENOMEM)?;
            process.descriptors.push(None);
            process.descriptors.len() - 1
        }
    };
    let size = match fs::with_vfs(|vfs| {
        let mut created = false;
        let metadata = match vfs.metadata(&path) {
            Ok(metadata) => metadata,
            Err(FsError::NotFound) if flags & O_CREAT != 0 => {
                vfs.create_file(&path)?;
                vfs.set_permissions(&path, mode)?;
                created = true;
                vfs.metadata(&path)?
            }
            Err(error) => return Err(error),
        };
        if metadata.node_type == NodeType::Directory {
            return Err(FsError::IsDirectory);
        }
        if !created
            && ((readable && metadata.permissions & 0o400 == 0)
                || (writable && metadata.permissions & 0o200 == 0))
        {
            return Err(FsError::PermissionDenied);
        }
        if flags & O_TRUNC != 0 {
            vfs.truncate(&path, 0)?;
            Ok(0)
        } else {
            Ok(metadata.len)
        }
    })
    .map_err(errno_from_fs)?
    .map_err(errno_from_fs)
    {
        Ok(size) => size,
        Err(errno) => {
            USER_PROCESS.lock().descriptors[fd] = None;
            return Err(errno);
        }
    };

    let mut process = USER_PROCESS.lock();
    process.descriptors[fd] = Some(UserDescriptor::File(OpenFile {
        path,
        offset: if flags & O_APPEND != 0 { size } else { 0 },
        readable,
        writable,
        append: flags & O_APPEND != 0,
    }));
    Ok(fd as u64)
}

fn syscall_pipe(address: u64) -> Result<u64, i32> {
    paging::validate_user_range(address, core::mem::size_of::<[i32; 2]>(), true)
        .map_err(|_| EFAULT)?;
    let (read_end, write_end) = create_pipe()?;
    let (read_fd, write_fd) = {
        let mut process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        install_pipe_descriptors(&mut process.descriptors, read_end, write_end)?
    };
    unsafe {
        (address as *mut [i32; 2]).write_unaligned([read_fd as i32, write_fd as i32]);
    }
    Ok(0)
}

fn syscall_dup2(source_fd: u64, target_fd: u64) -> Result<u64, i32> {
    let source_fd = usize::try_from(source_fd).map_err(|_| EBADF)?;
    let target_fd = usize::try_from(target_fd).map_err(|_| EBADF)?;
    if target_fd >= MAX_OPEN_FILES {
        return Err(EBADF);
    }
    if source_fd == target_fd {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        return process
            .descriptors
            .get(source_fd)
            .and_then(Option::as_ref)
            .map(|_| target_fd as u64)
            .ok_or(EBADF);
    }

    let replaced_pipe_id = {
        let mut process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        let duplicate = process
            .descriptors
            .get(source_fd)
            .and_then(Option::as_ref)
            .ok_or(EBADF)?
            .clone();
        let replaced_pipe_id = process
            .descriptors
            .get(target_fd)
            .and_then(Option::as_ref)
            .and_then(|descriptor| match descriptor {
                UserDescriptor::Pipe(endpoint) => Some(endpoint.pipe.lock().id),
                _ => None,
            });
        if target_fd >= process.descriptors.len() {
            let additional = target_fd + 1 - process.descriptors.len();
            process
                .descriptors
                .try_reserve(additional)
                .map_err(|_| ENOMEM)?;
            process.descriptors.resize_with(target_fd + 1, || None);
        }
        process.descriptors[target_fd] = Some(duplicate);
        replaced_pipe_id
    };
    if let Some(pipe_id) = replaced_pipe_id {
        wake_pipe_waiters(pipe_id, true, true);
    }
    Ok(target_fd as u64)
}

fn syscall_close(fd: u64) -> Result<u64, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let descriptor = process.descriptors.get_mut(fd).ok_or(EBADF)?;
    if descriptor.is_none() {
        return Err(EBADF);
    }
    let pipe_id = descriptor.as_ref().and_then(|descriptor| match descriptor {
        UserDescriptor::Pipe(endpoint) => Some(endpoint.pipe.lock().id),
        _ => None,
    });
    *descriptor = None;
    drop(process);
    if let Some(pipe_id) = pipe_id {
        wake_pipe_waiters(pipe_id, true, true);
    }
    Ok(0)
}

fn syscall_lseek(fd: u64, offset: i64, whence: u64) -> Result<u64, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let file = match process.descriptors.get_mut(fd).and_then(Option::as_mut) {
        Some(UserDescriptor::File(file)) => file,
        Some(UserDescriptor::Pipe(_) | UserDescriptor::Directory(_)) => return Err(ESPIPE),
        _ => return Err(EBADF),
    };
    if fs::is_tty_path(&file.path) {
        return Err(ESPIPE);
    }
    let base = match whence {
        0 => 0usize,
        1 => file.offset,
        2 => fs::with_vfs(|vfs| vfs.metadata(&file.path).map(|metadata| metadata.len))
            .map_err(errno_from_fs)?
            .map_err(errno_from_fs)?,
        _ => return Err(EINVAL),
    };
    let result = (base as i128) + (offset as i128);
    if result < 0 || result > usize::MAX as i128 {
        return Err(EINVAL);
    }
    file.offset = result as usize;
    Ok(file.offset as u64)
}

fn syscall_opendir(address: u64) -> Result<u64, i32> {
    let path = resolve_process_path(&copy_user_path(address)?)?;
    let entries = fs::with_vfs(|vfs| vfs.read_dir(&path))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let fd = reserve_descriptor_slot(&mut process.descriptors)?;
    process.descriptors[fd] = Some(UserDescriptor::Directory(Arc::new(Mutex::new(
        DirectoryStream {
            entries,
            next_entry: 0,
        },
    ))));
    Ok(fd as u64)
}

fn syscall_readdir(fd: u64, address: u64) -> Result<u64, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let directory = {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        match process.descriptors.get(fd).and_then(Option::as_ref) {
            Some(UserDescriptor::Directory(directory)) => Arc::clone(directory),
            Some(_) => return Err(ENOTDIR),
            None => return Err(EBADF),
        }
    };

    paging::validate_user_range(address, core::mem::size_of::<UserDirent>(), true)
        .map_err(|_| EFAULT)?;
    let mut directory = directory.lock();
    let Some(entry) = directory.entries.get(directory.next_entry) else {
        return Ok(0);
    };
    let record = user_dirent(directory.next_entry, entry)?;
    directory.next_entry += 1;
    unsafe {
        core::ptr::write_unaligned(address as *mut UserDirent, record);
    }
    Ok(1)
}

fn user_dirent(index: usize, entry: &DirectoryEntry) -> Result<UserDirent, i32> {
    if entry.name.len() >= 256 {
        return Err(ENAMETOOLONG);
    }
    let offset = i64::try_from(index.checked_add(1).ok_or(EOVERFLOW)?).map_err(|_| EOVERFLOW)?;
    let file_type = match entry.metadata.node_type {
        NodeType::File => 8,
        NodeType::Directory => 4,
        NodeType::Symlink => 10,
    };
    let mut record = UserDirent {
        inode: 0,
        offset,
        record_length: u16::try_from(core::mem::size_of::<UserDirent>()).map_err(|_| EOVERFLOW)?,
        file_type,
        name: [0; 256],
    };
    record.name[..entry.name.len()].copy_from_slice(entry.name.as_bytes());
    Ok(record)
}

fn syscall_stat(path_address: u64, stat_address: u64) -> Result<u64, i32> {
    paging::validate_user_range(stat_address, core::mem::size_of::<UserStat>(), true)
        .map_err(|_| EFAULT)?;
    let path = resolve_process_path(&copy_user_path(path_address)?)?;
    let metadata = fs::with_vfs(|vfs| vfs.metadata(&path))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    let stat = UserStat {
        size: metadata.len as u64,
        mode: stat_mode(&path, metadata.node_type, metadata.permissions),
        reserved: 0,
    };
    unsafe {
        core::ptr::write_unaligned(stat_address as *mut UserStat, stat);
    }
    Ok(0)
}

fn syscall_mkdir(path_address: u64) -> Result<u64, i32> {
    let path = resolve_process_path(&copy_user_path(path_address)?)?;
    fs::with_vfs(|vfs| vfs.create_dir(&path))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    Ok(0)
}

fn syscall_unlink(path_address: u64) -> Result<u64, i32> {
    let path = resolve_process_path(&copy_user_path(path_address)?)?;
    fs::with_vfs(|vfs| vfs.remove(&path))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    Ok(0)
}

fn syscall_ftruncate(fd: u64, length: u64) -> Result<u64, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let length = usize::try_from(length).map_err(|_| EINVAL)?;
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let file = match process.descriptors.get_mut(fd).and_then(Option::as_mut) {
        Some(UserDescriptor::File(file)) if file.writable => file,
        _ => return Err(EBADF),
    };
    fs::with_vfs(|vfs| vfs.truncate(&file.path, length))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    if file.offset > length {
        file.offset = length;
    }
    Ok(0)
}

fn syscall_brk(address: u64) -> Result<u64, i32> {
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    if address == 0 {
        return Ok(process.program_break);
    }
    let target_mapped_end = heap_mapping_end(address, process.heap_limit)?;

    if target_mapped_end > process.heap_mapped_end {
        let additional_pages =
            usize::try_from((target_mapped_end - process.heap_mapped_end) / PAGE_SIZE)
                .map_err(|_| ENOMEM)?;
        process
            .heap_pages
            .try_reserve_exact(additional_pages)
            .map_err(|_| ENOMEM)?;
        let initial_page_count = process.heap_pages.len();
        let mut page = process.heap_mapped_end;
        while page < target_mapped_end {
            if let Err(error) = map_zeroed_page(
                page,
                paging::flags::USER | paging::flags::WRITABLE | paging::flags::NO_EXECUTE,
                &mut process.heap_pages,
            ) {
                let cleanup =
                    release_tracked_pages_from(&mut process.heap_pages, initial_page_count);
                return Err(if cleanup.is_err() {
                    EIO
                } else {
                    match error {
                        ElfError::OutOfMemory => ENOMEM,
                        ElfError::Paging(error) => paging_errno(error),
                        _ => EIO,
                    }
                });
            }
            page += PAGE_SIZE;
        }
        process.heap_mapped_end = target_mapped_end;
    } else if target_mapped_end < process.heap_mapped_end {
        while process.heap_mapped_end > target_mapped_end {
            let page = process.heap_mapped_end - PAGE_SIZE;
            if process.heap_pages.last().copied() != Some(page) {
                return Err(EIO);
            }
            let physical = unsafe { paging::unmap_page(page) }.map_err(|_| EIO)?;
            free_frame(physical as usize);
            process.heap_pages.pop();
            process.heap_mapped_end = page;
        }
    }

    process.program_break = address;
    Ok(address)
}

fn heap_mapping_end(address: u64, heap_limit: u64) -> Result<u64, i32> {
    if !(HEAP_START..=heap_limit).contains(&address) {
        return Err(ENOMEM);
    }
    let mapped_end = address.checked_add(PAGE_SIZE - 1).ok_or(ENOMEM)? & !(PAGE_SIZE - 1);
    let mapped_end = mapped_end.max(HEAP_INITIAL_END);
    if mapped_end > heap_limit {
        return Err(ENOMEM);
    }
    Ok(mapped_end)
}

fn syscall_mmap(address: u64, length: u64, protection: u64) -> Result<u64, i32> {
    let (mapped_size, page_count) = mapping_size(length)?;
    let page_flags = mapping_flags(protection)?;
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }

    let start = if address == 0 {
        let end = process.mmap_cursor.checked_add(mapped_size).ok_or(ENOMEM)?;
        if process.mmap_cursor < paging::USER_ADDRESS_MIN || end > MMAP_END {
            return Err(ENOMEM);
        }
        process.mmap_cursor
    } else {
        if address % PAGE_SIZE != 0 {
            return Err(EINVAL);
        }
        validate_mapping_range(address, mapped_size)?;
        address
    };
    let end = start.checked_add(mapped_size).ok_or(ENOMEM)?;
    process
        .mmap_pages
        .try_reserve_exact(page_count)
        .map_err(|_| ENOMEM)?;
    let initial_page_count = process.mmap_pages.len();

    for index in 0..page_count {
        let page = start
            .checked_add(
                u64::try_from(index)
                    .map_err(|_| ENOMEM)?
                    .checked_mul(PAGE_SIZE)
                    .ok_or(ENOMEM)?,
            )
            .ok_or(ENOMEM)?;
        let Some(physical) = allocate_frame() else {
            let cleanup = release_tracked_pages_from(&mut process.mmap_pages, initial_page_count);
            return Err(if cleanup.is_err() { EIO } else { ENOMEM });
        };
        let physical = physical as u64;
        if let Err(error) = unsafe { paging::map_page(page, physical, page_flags) } {
            free_frame(physical as usize);
            let cleanup = release_tracked_pages_from(&mut process.mmap_pages, initial_page_count);
            return Err(if cleanup.is_err() {
                EIO
            } else {
                paging_errno(error)
            });
        }
        process.mmap_pages.push(page);
    }

    process.mmap_pages.sort_unstable();
    process.mmap_cursor = process.mmap_cursor.max(end);
    Ok(start)
}

fn syscall_mmap_file(fd: u64, offset: u64, length: u64) -> Result<u64, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let path = {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        let file = match process.descriptors.get(fd).and_then(Option::as_ref) {
            Some(UserDescriptor::File(file)) if file.readable => file,
            _ => return Err(EBADF),
        };
        let mut path = String::new();
        path.try_reserve_exact(file.path.len())
            .map_err(|_| ENOMEM)?;
        path.push_str(&file.path);
        path
    };

    mapping_size(length)?;
    let metadata = fs::with_vfs(|vfs| vfs.metadata(&path))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    if metadata.node_type != NodeType::File {
        return Err(EINVAL);
    }
    let file_size = u64::try_from(metadata.len).map_err(|_| EINVAL)?;
    validate_file_mapping_range(offset, length, file_size)?;
    let file_offset = usize::try_from(offset).map_err(|_| EINVAL)?;
    let file_length = usize::try_from(length).map_err(|_| EINVAL)?;
    file_offset.checked_add(file_length).ok_or(EINVAL)?;

    let address = syscall_mmap(0, length, PROT_READ)?;
    let mut copied = 0usize;
    let mut page_buffer = [0; PAGE_SIZE as usize];
    while copied < file_length {
        let chunk = (file_length - copied).min(PAGE_SIZE as usize);
        let read_offset = file_offset + copied;
        let read = match fs::with_vfs(|vfs| vfs.read(&path, read_offset, &mut page_buffer[..chunk]))
        {
            Ok(Ok(read)) => read,
            Ok(Err(error)) | Err(error) => {
                return discard_mapping(address, length, errno_from_fs(error));
            }
        };
        if read != chunk {
            return discard_mapping(address, length, EIO);
        }
        let destination = match address.checked_add(copied as u64) {
            Some(destination) => destination,
            None => return discard_mapping(address, length, EIO),
        };
        if copy_to_user_pages(destination, &page_buffer[..chunk]).is_err() {
            return discard_mapping(address, length, EIO);
        }
        copied += chunk;
    }
    Ok(address)
}

fn discard_mapping(address: u64, length: u64, error: i32) -> Result<u64, i32> {
    if syscall_munmap(address, length).is_err() {
        Err(EIO)
    } else {
        Err(error)
    }
}

fn syscall_munmap(address: u64, length: u64) -> Result<u64, i32> {
    if address == 0 || address % PAGE_SIZE != 0 {
        return Err(EINVAL);
    }
    let (mapped_size, page_count) = mapping_size(length)?;
    validate_mapping_range(address, mapped_size)?;
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }

    for index in 0..page_count {
        let page = address
            .checked_add(
                u64::try_from(index)
                    .map_err(|_| EINVAL)?
                    .checked_mul(PAGE_SIZE)
                    .ok_or(EINVAL)?,
            )
            .ok_or(EINVAL)?;
        if process.mmap_pages.binary_search(&page).is_err() {
            return Err(EINVAL);
        }
    }
    let first_page = process
        .mmap_pages
        .binary_search(&address)
        .map_err(|_| EINVAL)?;
    for index in 0..page_count {
        let page = address
            .checked_add(
                u64::try_from(index)
                    .map_err(|_| EINVAL)?
                    .checked_mul(PAGE_SIZE)
                    .ok_or(EINVAL)?,
            )
            .ok_or(EINVAL)?;
        let physical = match unsafe { paging::unmap_page(page) } {
            Ok(physical) => physical,
            Err(_) => {
                process.mmap_pages.retain(|mapped| *mapped != 0);
                return Err(EIO);
            }
        };
        free_frame(physical as usize);
        process.mmap_pages[first_page + index] = 0;
    }
    process.mmap_pages.retain(|mapped| *mapped != 0);
    Ok(0)
}

fn mapping_size(length: u64) -> Result<(u64, usize), i32> {
    if length == 0 {
        return Err(EINVAL);
    }
    let mapped_size = length.checked_add(PAGE_SIZE - 1).ok_or(EINVAL)? & !(PAGE_SIZE - 1);
    let page_count = usize::try_from(mapped_size / PAGE_SIZE).map_err(|_| ENOMEM)?;
    Ok((mapped_size, page_count))
}

fn validate_file_mapping_range(offset: u64, length: u64, file_size: u64) -> Result<(), i32> {
    if length == 0 || offset % PAGE_SIZE != 0 {
        return Err(EINVAL);
    }
    let end = offset.checked_add(length).ok_or(EINVAL)?;
    if end > file_size {
        return Err(EINVAL);
    }
    Ok(())
}

fn mapping_flags(protection: u64) -> Result<u64, i32> {
    if protection & !(PROT_READ | PROT_WRITE | PROT_EXEC) != 0
        || protection & PROT_READ == 0
        || protection & (PROT_WRITE | PROT_EXEC) == (PROT_WRITE | PROT_EXEC)
    {
        return Err(EINVAL);
    }
    let mut flags = paging::flags::USER;
    if protection & PROT_WRITE != 0 {
        flags |= paging::flags::WRITABLE;
    }
    if protection & PROT_EXEC == 0 {
        flags |= paging::flags::NO_EXECUTE;
    }
    Ok(flags)
}

fn validate_mapping_range(address: u64, length: u64) -> Result<(), i32> {
    let end = address.checked_add(length).ok_or(EINVAL)?;
    if address < paging::USER_ADDRESS_MIN || end > MMAP_END {
        return Err(EINVAL);
    }
    Ok(())
}

fn release_tracked_pages_from(mapped_pages: &mut Vec<u64>, first_page: usize) -> Result<(), i32> {
    while mapped_pages.len() > first_page {
        let page = *mapped_pages.last().ok_or(EIO)?;
        let physical = unsafe { paging::unmap_page(page) }.map_err(|_| EIO)?;
        mapped_pages.pop();
        free_frame(physical as usize);
    }
    Ok(())
}

fn paging_errno(error: PagingError) -> i32 {
    match error {
        PagingError::AlreadyMapped => EEXIST,
        PagingError::OutOfFrames | PagingError::HhdmUnavailable => ENOMEM,
        _ => EINVAL,
    }
}

fn syscall_chdir(path_address: u64) -> Result<u64, i32> {
    let path = resolve_process_path(&copy_user_path(path_address)?)?;
    let metadata = fs::with_vfs(|vfs| vfs.metadata(&path))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    if metadata.node_type != NodeType::Directory {
        return Err(ENOTDIR);
    }
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    process.cwd = path;
    Ok(0)
}

fn syscall_chmod(path_address: u64, mode: u64) -> Result<u64, i32> {
    let mode = u16::try_from(mode).map_err(|_| EINVAL)?;
    if mode & !0o777 != 0 {
        return Err(EINVAL);
    }
    let path = resolve_process_path(&copy_user_path(path_address)?)?;
    fs::with_vfs(|vfs| vfs.set_permissions(&path, mode))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    Ok(0)
}

fn syscall_getcwd(address: u64, size: u64) -> Result<u64, i32> {
    let size = usize::try_from(size).map_err(|_| EINVAL)?;
    if size == 0 {
        return Err(EINVAL);
    }
    let path = {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        process.cwd.clone()
    };
    let needed = path.len().checked_add(1).ok_or(EINVAL)?;
    if size < needed {
        return Err(ERANGE);
    }
    paging::validate_user_range(address, needed, true).map_err(|_| EFAULT)?;
    unsafe {
        core::ptr::copy_nonoverlapping(path.as_ptr(), address as *mut u8, path.len());
        (address as *mut u8).add(path.len()).write(0);
    }
    Ok(needed as u64)
}

fn syscall_putpixel(x: u64, y: u64, color: u64) -> Result<u64, i32> {
    let framebuffer = crate::graphics::Framebuffer::get().ok_or(ENODEV)?;
    let color = validate_pixel(x, y, color, framebuffer.width(), framebuffer.height())?;
    framebuffer.write_pixel(x, y, crate::graphics::Color(color));
    Ok(0)
}

fn validate_pixel(x: u64, y: u64, color: u64, width: u64, height: u64) -> Result<u32, i32> {
    if color > 0x00FF_FFFF || x >= width || y >= height {
        return Err(EINVAL);
    }
    Ok(color as u32)
}

fn resolve_process_path(path: &str) -> Result<String, i32> {
    let cwd = {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        process.cwd.clone()
    };
    resolve_user_path(&cwd, path).map_err(errno_from_fs)
}

fn resolve_user_path(cwd: &str, path: &str) -> Result<String, FsError> {
    if path.is_empty() {
        return Err(FsError::InvalidPath);
    }
    if path.starts_with('/') {
        return fs::normalize_path(path);
    }

    let capacity = cwd
        .len()
        .checked_add(path.len())
        .and_then(|length| length.checked_add(1))
        .ok_or(FsError::NoSpace)?;
    let mut joined = String::new();
    joined.try_reserve(capacity).map_err(|_| FsError::NoSpace)?;
    joined.push_str(cwd);
    if !cwd.ends_with('/') {
        joined.push('/');
    }
    joined.push_str(path);
    fs::normalize_path(&joined)
}

fn copy_user_path(address: u64) -> Result<String, i32> {
    copy_user_string(address, MAX_USER_PATH)
}

fn copy_user_arguments(address: u64, count: u64) -> Result<Vec<String>, i32> {
    let count = usize::try_from(count).map_err(|_| EINVAL)?;
    if count > MAX_ARGUMENTS {
        return Err(EINVAL);
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    let pointer_bytes = count
        .checked_mul(core::mem::size_of::<u64>())
        .ok_or(EINVAL)?;
    if address == 0 {
        return Err(EFAULT);
    }
    paging::validate_user_range(address, pointer_bytes, false).map_err(|_| EFAULT)?;

    let mut arguments = Vec::new();
    arguments.try_reserve_exact(count).map_err(|_| ENOMEM)?;
    let mut total_bytes = 0usize;
    for index in 0..count {
        let pointer_address = address
            .checked_add((index * core::mem::size_of::<u64>()) as u64)
            .ok_or(EFAULT)?;
        let argument_address = unsafe { (pointer_address as *const u64).read_unaligned() };
        let argument = copy_user_string(argument_address, MAX_ARGUMENT_BYTES)?;
        total_bytes = total_bytes
            .checked_add(argument.len().checked_add(1).ok_or(EINVAL)?)
            .filter(|length| *length <= MAX_ARGUMENT_BYTES)
            .ok_or(EINVAL)?;
        arguments.push(argument);
    }
    Ok(arguments)
}

fn copy_user_environment(address: u64) -> Result<Vec<String>, i32> {
    if address == 0 {
        return Err(EFAULT);
    }
    let mut environment = Vec::new();
    environment
        .try_reserve_exact(MAX_ENVIRONMENT_VARIABLES)
        .map_err(|_| ENOMEM)?;
    let mut total_bytes = 0usize;
    for index in 0..=MAX_ENVIRONMENT_VARIABLES {
        let pointer_address = address
            .checked_add((index * core::mem::size_of::<u64>()) as u64)
            .ok_or(EFAULT)?;
        paging::validate_user_range(pointer_address, core::mem::size_of::<u64>(), false)
            .map_err(|_| EFAULT)?;
        let entry_address = unsafe { (pointer_address as *const u64).read_unaligned() };
        if entry_address == 0 {
            return Ok(environment);
        }
        if index == MAX_ENVIRONMENT_VARIABLES {
            return Err(E2BIG);
        }

        let entry = copy_user_string(entry_address, MAX_ENVIRONMENT_BYTES)?;
        let (key, _) = entry.split_once('=').ok_or(EINVAL)?;
        if !valid_environment_key(key) {
            return Err(EINVAL);
        }
        total_bytes = total_bytes
            .checked_add(entry.len().checked_add(1).ok_or(E2BIG)?)
            .filter(|length| *length <= MAX_ENVIRONMENT_BYTES)
            .ok_or(E2BIG)?;
        environment.push(entry);
    }
    Err(E2BIG)
}

fn copy_user_string(address: u64, maximum_length: usize) -> Result<String, i32> {
    if address == 0 {
        return Err(EFAULT);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve(maximum_length.min(64))
        .map_err(|_| ENOMEM)?;
    for index in 0..maximum_length {
        let current = address.checked_add(index as u64).ok_or(EFAULT)?;
        paging::validate_user_range(current, 1, false).map_err(|_| EFAULT)?;
        let byte = unsafe { (current as *const u8).read() };
        if byte == 0 {
            return String::from_utf8(bytes).map_err(|_| EILSEQ);
        }
        if bytes.len() == bytes.capacity() {
            bytes.try_reserve(1).map_err(|_| ENOMEM)?;
        }
        bytes.push(byte);
    }
    Err(ENAMETOOLONG)
}

fn errno_from_fs(error: FsError) -> i32 {
    match error {
        FsError::NotFound => ENOENT,
        FsError::AlreadyExists => EEXIST,
        FsError::NotDirectory => ENOTDIR,
        FsError::IsDirectory => EISDIR,
        FsError::InvalidPath => EINVAL,
        FsError::PermissionDenied => EACCES,
        FsError::InvalidData => EILSEQ,
        FsError::NoSpace => ENOSPC,
        FsError::ReadOnly => EROFS,
        FsError::DirectoryNotEmpty => ENOTEMPTY,
        FsError::SymlinkLoop => ELOOP,
        FsError::Unsupported => ENOSYS,
        _ => EIO,
    }
}

fn stat_mode(path: &str, node_type: NodeType, permissions: u16) -> u32 {
    let node_mode = if fs::is_tty_path(path) {
        0o020000
    } else {
        match node_type {
            NodeType::File => 0o100000,
            NodeType::Directory => 0o040000,
            NodeType::Symlink => 0o120000,
        }
    };
    node_mode | u32::from(permissions & 0o777)
}

fn next_runnable(
    process_count: usize,
    current: usize,
    mut is_runnable: impl FnMut(usize) -> bool,
) -> Option<usize> {
    if process_count == 0 || current >= process_count {
        return None;
    }
    for offset in 1..=process_count {
        let candidate = (current + offset) % process_count;
        if is_runnable(candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_process_runnable(process: &ScheduledProcess) -> bool {
    process.exit_status.is_none() && process.waiting_for.is_none()
}

pub fn terminate_user(frame: &mut InterruptFrame, status: u64) -> *mut InterruptFrame {
    let next_frame = terminate_user_inner(frame, status);
    restore_current_user_fpu();
    next_frame
}

fn terminate_user_inner(frame: &mut InterruptFrame, status: u64) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    if frame.stack_frame.code_segment & 3 != 3 {
        return frame_pointer;
    }

    let mut scheduler = USER_SCHEDULER.lock();
    if scheduler.active && !scheduler.processes.is_empty() {
        let current = scheduler.current;
        if current < scheduler.processes.len() {
            let child_pid = scheduler.processes[current].pid;
            let parent_pid = scheduler.processes[current].parent_pid;
            scheduler.processes[current].exit_status = Some(status);
            let mut active_process = USER_PROCESS.lock();
            core::mem::swap(
                &mut *active_process,
                &mut scheduler.processes[current].state,
            );
            scheduler.processes[current].state.active = false;
            while let Some(descriptor) = scheduler.processes[current].state.descriptors.pop() {
                let pipe_id = descriptor.as_ref().and_then(|descriptor| match descriptor {
                    UserDescriptor::Pipe(endpoint) => Some(endpoint.pipe.lock().id),
                    _ => None,
                });
                drop(descriptor);
                if let Some(pipe_id) = pipe_id {
                    wake_pipe_waiters_in_scheduler(&mut scheduler, pipe_id, true, true);
                }
            }

            if let Some(parent) = parent_pid.and_then(|pid| {
                scheduler.processes.iter().position(|process| {
                    process.pid == pid
                        && matches!(
                            process.waiting_for,
                            Some(ProcessWait::Child(target))
                                if wait_target_matches(target, child_pid)
                        )
                })
            }) {
                let status_pointer = scheduler.processes[parent].wait_status_pointer;
                scheduler.processes[parent].waiting_for = None;
                scheduler.processes[parent].wait_status_pointer = 0;
                core::mem::swap(&mut *active_process, &mut scheduler.processes[parent].state);
                scheduler.current = parent;
                let parent_process = &scheduler.processes[parent];
                let parent_frame = parent_process.frame_pointer as *mut InterruptFrame;
                unsafe {
                    gdt::set_ring0_stack_top(parent_process.kernel_stack_top);
                    parent_process.image.address_space.activate();
                    (*parent_frame).rax = if status_pointer == 0 {
                        child_pid
                    } else {
                        match write_user_u64(status_pointer, status) {
                            Ok(()) => child_pid,
                            Err(error) => (-(error as i64)) as u64,
                        }
                    };
                }
                if status_pointer == 0
                    || paging::validate_user_range(
                        status_pointer,
                        core::mem::size_of::<u64>(),
                        true,
                    )
                    .is_ok()
                {
                    scheduler.processes[current].reaped = true;
                }
                return parent_frame;
            }

            if let Some(next) = next_runnable(scheduler.processes.len(), current, |index| {
                is_process_runnable(&scheduler.processes[index])
            }) {
                core::mem::swap(&mut *active_process, &mut scheduler.processes[next].state);
                scheduler.current = next;
                let next_process = &scheduler.processes[next];
                let next_frame = next_process.frame_pointer as *mut InterruptFrame;
                unsafe {
                    gdt::set_ring0_stack_top(next_process.kernel_stack_top);
                    next_process.image.address_space.activate();
                }
                return next_frame;
            }
            scheduler.active = false;
            drop(active_process);
            drop(scheduler);
            gdt::record_user_exit(status);
            frame.stack_frame.instruction_pointer = gdt::user_exit_address();
            frame.stack_frame.code_segment = gdt::KERNEL_CODE_SELECTOR as u64;
            frame.stack_frame.stack_pointer = gdt::user_kernel_stack_top();
            frame.stack_frame.stack_segment = gdt::KERNEL_DATA_SELECTOR as u64;
            return frame_pointer;
        }
    }
    drop(scheduler);

    gdt::record_user_exit(status);
    frame.stack_frame.instruction_pointer = gdt::user_exit_address();
    frame.stack_frame.code_segment = gdt::KERNEL_CODE_SELECTOR as u64;
    frame.stack_frame.stack_pointer = gdt::user_kernel_stack_top();
    frame.stack_frame.stack_segment = gdt::KERNEL_DATA_SELECTOR as u64;
    frame_pointer
}

fn map_zeroed_page(
    virtual_address: u64,
    flags: u64,
    mapped_pages: &mut Vec<u64>,
) -> Result<(), ElfError> {
    let physical = allocate_frame().ok_or(ElfError::OutOfMemory)? as u64;
    if let Err(error) = unsafe { paging::map_page(virtual_address, physical, flags) } {
        free_frame(physical as usize);
        return Err(ElfError::Paging(error));
    }
    mapped_pages.push(virtual_address);
    Ok(())
}

fn copy_to_user_pages(address: u64, source: &[u8]) -> Result<(), ElfError> {
    let mut copied = 0;
    while copied < source.len() {
        let current = address
            .checked_add(copied as u64)
            .ok_or(ElfError::AddressRange)?;
        let physical = paging::translate_address(current)?;
        let destination = crate::memory::physical_to_virtual(physical as usize)
            .ok_or(PagingError::HhdmUnavailable)?;
        let page_remaining = (PAGE_SIZE - current % PAGE_SIZE) as usize;
        let chunk = (source.len() - copied).min(page_remaining);
        unsafe {
            core::ptr::copy_nonoverlapping(
                source.as_ptr().add(copied),
                destination as *mut u8,
                chunk,
            );
        }
        copied += chunk;
    }
    Ok(())
}

fn release_pages(mapped_pages: &mut Vec<u64>) -> Result<(), ElfError> {
    let mut first_error = None;
    for virtual_address in mapped_pages.drain(..) {
        match unsafe { paging::unmap_page(virtual_address) } {
            Ok(physical) => free_frame(physical as usize),
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    first_error.map_or(Ok(()), |error| Err(ElfError::Paging(error)))
}

fn parse(bytes: &[u8]) -> Result<(u64, Vec<LoadSegment>), ElfError> {
    if bytes.len() < ELF_HEADER_SIZE {
        return Err(ElfError::InvalidHeader);
    }
    if bytes.get(..4) != Some(b"\x7FELF") || bytes[4] != 2 || bytes[5] != 1 || bytes[6] != 1 {
        return Err(ElfError::UnsupportedFormat);
    }
    if read_u16(bytes, 16)? != 2
        || read_u16(bytes, 18)? != 62
        || read_u32(bytes, 20)? != 1
        || read_u16(bytes, 52)? != ELF_HEADER_SIZE as u16
    {
        return Err(ElfError::UnsupportedFormat);
    }

    let entry = read_u64(bytes, 24)?;
    let ph_offset = usize::try_from(read_u64(bytes, 32)?).map_err(|_| ElfError::OutOfBounds)?;
    let ph_entry_size = read_u16(bytes, 54)? as usize;
    let ph_count = read_u16(bytes, 56)? as usize;
    if ph_offset < ELF_HEADER_SIZE
        || ph_entry_size < PROGRAM_HEADER_SIZE
        || ph_count == 0
        || ph_count > MAX_PROGRAM_HEADERS
    {
        return Err(ElfError::InvalidHeader);
    }
    let ph_bytes = ph_entry_size
        .checked_mul(ph_count)
        .ok_or(ElfError::OutOfBounds)?;
    let ph_end = ph_offset
        .checked_add(ph_bytes)
        .ok_or(ElfError::OutOfBounds)?;
    if ph_end > bytes.len() {
        return Err(ElfError::OutOfBounds);
    }

    let mut segments = Vec::new();
    segments
        .try_reserve_exact(ph_count)
        .map_err(|_| ElfError::OutOfMemory)?;
    for index in 0..ph_count {
        let offset = ph_offset + index * ph_entry_size;
        match read_u32(bytes, offset)? {
            PT_DYNAMIC | PT_INTERP => return Err(ElfError::DynamicLinkingUnsupported),
            PT_LOAD => {
                let flags = read_u32(bytes, offset + 4)?;
                let file_offset = usize::try_from(read_u64(bytes, offset + 8)?)
                    .map_err(|_| ElfError::OutOfBounds)?;
                let virtual_address = read_u64(bytes, offset + 16)?;
                let file_size = usize::try_from(read_u64(bytes, offset + 32)?)
                    .map_err(|_| ElfError::OutOfBounds)?;
                let memory_size = read_u64(bytes, offset + 40)?;
                let alignment = read_u64(bytes, offset + 48)?;
                if memory_size == 0 && file_size == 0 {
                    continue;
                }
                if file_size as u64 > memory_size {
                    return Err(ElfError::InvalidSegment);
                }
                if flags & (PF_W | PF_X) == (PF_W | PF_X) {
                    return Err(ElfError::WritableExecutableSegment);
                }
                if alignment > 1
                    && (!alignment.is_power_of_two()
                        || virtual_address % alignment != (file_offset as u64) % alignment)
                {
                    return Err(ElfError::InvalidSegment);
                }
                let file_end = file_offset
                    .checked_add(file_size)
                    .ok_or(ElfError::OutOfBounds)?;
                if file_end > bytes.len() {
                    return Err(ElfError::OutOfBounds);
                }
                let memory_end = virtual_address
                    .checked_add(memory_size)
                    .ok_or(ElfError::AddressRange)?;
                if virtual_address < paging::USER_ADDRESS_MIN
                    || memory_end > paging::USER_ADDRESS_MAX
                {
                    return Err(ElfError::AddressRange);
                }
                segments.push(LoadSegment {
                    file_offset,
                    virtual_address,
                    file_size,
                    memory_size,
                    flags,
                });
            }
            _ => {}
        }
    }
    if segments.is_empty() {
        return Err(ElfError::InvalidSegment);
    }
    Ok((entry, segments))
}

fn page_start(address: u64) -> u64 {
    address & !(PAGE_SIZE - 1)
}

fn page_end(address: u64, size: u64) -> Result<u64, ElfError> {
    let end = address.checked_add(size).ok_or(ElfError::AddressRange)?;
    end.checked_add(PAGE_SIZE - 1)
        .map(|aligned| aligned & !(PAGE_SIZE - 1))
        .ok_or(ElfError::AddressRange)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ElfError> {
    let end = offset.checked_add(2).ok_or(ElfError::OutOfBounds)?;
    let value: [u8; 2] = bytes
        .get(offset..end)
        .ok_or(ElfError::OutOfBounds)?
        .try_into()
        .map_err(|_| ElfError::OutOfBounds)?;
    Ok(u16::from_le_bytes(value))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ElfError> {
    let end = offset.checked_add(4).ok_or(ElfError::OutOfBounds)?;
    let value: [u8; 4] = bytes
        .get(offset..end)
        .ok_or(ElfError::OutOfBounds)?
        .try_into()
        .map_err(|_| ElfError::OutOfBounds)?;
    Ok(u32::from_le_bytes(value))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, ElfError> {
    let end = offset.checked_add(8).ok_or(ElfError::OutOfBounds)?;
    let value: [u8; 8] = bytes
        .get(offset..end)
        .ok_or(ElfError::OutOfBounds)?
        .try_into()
        .map_err(|_| ElfError::OutOfBounds)?;
    Ok(u64::from_le_bytes(value))
}

#[cfg(test)]
mod tests {
    use super::{
        EACCES, EAGAIN, EEXIST, EILSEQ, EINTR, EINVAL, ENOENT, ENOMEM, ENOSYS, EPIPE, ElfError,
        HEAP_INITIAL_END, HEAP_START, MMAP_END, MMAP_START, NodeType, PIPE_CAPACITY, PROT_EXEC,
        PROT_READ, PROT_WRITE, Pipe, SYSCALL_BRK, SYSCALL_MMAP, SYSCALL_MMAP_FILE, SYSCALL_MUNMAP,
        UserDescriptor, UserDirent, WAIT_ANY_PID, build_initial_stack, create_pipe,
        dispatch_syscall, errno_from_fs, heap_mapping_end, install_pipe_descriptors, mapping_flags,
        mapping_size, next_runnable, parse, resolve_user_path, stat_mode, user_dirent,
        validate_file_mapping_range, validate_mapping_range, validate_pixel, wait_target_matches,
    };
    use crate::fs::{DirectoryEntry, FsError, Metadata};

    #[test]
    fn encodes_directory_entries_in_the_user_abi_layout() {
        let entry = DirectoryEntry {
            name: String::from("sample"),
            metadata: Metadata {
                node_type: NodeType::Directory,
                len: 0,
                permissions: 0o755,
            },
        };
        let record = user_dirent(2, &entry).unwrap();

        assert_eq!(core::mem::size_of::<UserDirent>(), 280);
        assert_eq!(core::mem::offset_of!(UserDirent, name), 19);
        assert_eq!(record.offset, 3);
        assert_eq!(
            record.record_length as usize,
            core::mem::size_of::<UserDirent>()
        );
        assert_eq!(record.file_type, 4);
        assert_eq!(&record.name[..7], b"sample\0");
    }

    #[test]
    fn rejects_directory_entry_names_that_do_not_fit_the_abi() {
        let entry = DirectoryEntry {
            name: String::from_utf8(alloc::vec![b'x'; 256]).unwrap(),
            metadata: Metadata {
                node_type: NodeType::File,
                len: 0,
                permissions: 0o644,
            },
        };

        assert!(matches!(
            user_dirent(0, &entry),
            Err(error) if error == super::ENAMETOOLONG
        ));
    }

    fn sample_elf() -> Vec<u8> {
        let mut bytes = alloc::vec![0; 0x1001];
        bytes[..16].copy_from_slice(b"\x7FELF\x02\x01\x01\0\0\0\0\0\0\0\0\0");
        put_u16(&mut bytes, 16, 2);
        put_u16(&mut bytes, 18, 62);
        put_u32(&mut bytes, 20, 1);
        put_u64(&mut bytes, 24, 0x400000);
        put_u64(&mut bytes, 32, 64);
        put_u16(&mut bytes, 52, 64);
        put_u16(&mut bytes, 54, 56);
        put_u16(&mut bytes, 56, 1);
        put_u32(&mut bytes, 64, 1);
        put_u32(&mut bytes, 68, 5);
        put_u64(&mut bytes, 72, 0x1000);
        put_u64(&mut bytes, 80, 0x400000);
        put_u64(&mut bytes, 96, 1);
        put_u64(&mut bytes, 104, 0x1000);
        put_u64(&mut bytes, 112, 0x1000);
        bytes[0x1000] = 0xC3;
        bytes
    }

    #[test]
    fn formats_printf_string_signed_and_hexadecimal_arguments() {
        let arguments = [24, 42, 1];
        let output = super::format_printf(
            "umur %d, hex %#x, %s\n",
            |index| arguments.get(index).copied().ok_or(EINVAL),
            |address, _| {
                if address == 1 {
                    Ok(alloc::string::String::from("siap"))
                } else {
                    Err(EINVAL)
                }
            },
        )
        .unwrap();
        assert_eq!(output, b"umur 24, hex 0x2a, siap\n");
    }

    #[test]
    fn formats_printf_width_and_rejects_unsupported_conversions() {
        let output = super::format_printf("%04d", |_| Ok(7), |_, _| Err(EINVAL)).unwrap();
        assert_eq!(output, b"0007");
        assert_eq!(
            super::format_printf("%.2f", |_| Ok(0), |_, _| Err(EINVAL)),
            Err(EINVAL)
        );
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

    #[test]
    fn parses_static_executable_segment() {
        let (entry, segments) = parse(&sample_elf()).unwrap();
        assert_eq!(entry, 0x400000);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].file_size, 1);
    }

    #[test]
    fn rejects_writable_executable_segments() {
        let mut bytes = sample_elf();
        put_u32(&mut bytes, 68, 7);
        assert_eq!(parse(&bytes), Err(ElfError::WritableExecutableSegment));
    }

    #[test]
    fn rejects_truncated_segment_data() {
        let mut bytes = sample_elf();
        put_u64(&mut bytes, 96, 2);
        assert_eq!(parse(&bytes), Err(ElfError::OutOfBounds));
    }

    #[test]
    fn rejects_dynamic_program_headers() {
        let mut bytes = sample_elf();
        put_u32(&mut bytes, 64, 2);
        assert_eq!(parse(&bytes), Err(ElfError::DynamicLinkingUnsupported));
    }

    #[test]
    fn maps_filesystem_errors_and_unknown_syscalls() {
        assert_eq!(errno_from_fs(FsError::NotFound), ENOENT);
        assert_eq!(errno_from_fs(FsError::AlreadyExists), EEXIST);
        assert_eq!(errno_from_fs(FsError::PermissionDenied), EACCES);
        assert_eq!(errno_from_fs(FsError::InvalidData), EILSEQ);
        assert_eq!(dispatch_syscall(u64::MAX, 0, 0, 0, 0), Err(ENOSYS));
    }

    #[test]
    fn validates_pixel_color_and_framebuffer_bounds() {
        assert_eq!(validate_pixel(2, 3, 0x123456, 8, 8), Ok(0x123456));
        assert_eq!(validate_pixel(8, 3, 0, 8, 8), Err(EINVAL));
        assert_eq!(validate_pixel(2, 8, 0, 8, 8), Err(EINVAL));
        assert_eq!(validate_pixel(2, 3, 0x1_000000, 8, 8), Err(EINVAL));
    }

    #[test]
    fn validates_anonymous_mapping_size_and_protection() {
        assert_eq!(mapping_size(1), Ok((4096, 1)));
        assert_eq!(mapping_size(4097), Ok((8192, 2)));
        assert_eq!(mapping_size(0), Err(EINVAL));
        assert_eq!(mapping_size(u64::MAX), Err(EINVAL));

        assert!(mapping_flags(PROT_READ).is_ok());
        assert!(mapping_flags(PROT_READ | PROT_WRITE).is_ok());
        assert!(mapping_flags(PROT_READ | PROT_EXEC).is_ok());
        assert_eq!(mapping_flags(0), Err(EINVAL));
        assert_eq!(
            mapping_flags(PROT_READ | PROT_WRITE | PROT_EXEC),
            Err(EINVAL)
        );
        assert_eq!(mapping_flags(PROT_WRITE), Err(EINVAL));
    }

    #[test]
    fn mmap_syscalls_require_an_active_user_process() {
        assert_eq!(
            dispatch_syscall(SYSCALL_MMAP, 0, 4096, PROT_READ, 0),
            Err(EINTR)
        );
        assert_eq!(
            dispatch_syscall(SYSCALL_MUNMAP, MMAP_START, 4096, 0, 0),
            Err(EINTR)
        );
        assert_eq!(
            dispatch_syscall(SYSCALL_MMAP_FILE, 3, 0, 4096, 0),
            Err(EINTR)
        );
        assert_eq!(
            dispatch_syscall(SYSCALL_BRK, HEAP_START + 1, 0, 0, 0),
            Err(EINTR)
        );
    }

    #[test]
    fn validates_read_only_file_mapping_bounds() {
        assert_eq!(validate_file_mapping_range(4096, 4096, 8192), Ok(()));
        assert_eq!(validate_file_mapping_range(1, 4096, 8192), Err(EINVAL));
        assert_eq!(validate_file_mapping_range(0, 0, 8192), Err(EINVAL));
        assert_eq!(validate_file_mapping_range(4096, 4097, 8192), Err(EINVAL));
        assert_eq!(
            validate_file_mapping_range(u64::MAX - 4095, 8192, u64::MAX),
            Err(EINVAL)
        );
    }

    #[test]
    fn computes_heap_mapping_growth_and_enforces_heap_limit() {
        assert_eq!(
            heap_mapping_end(HEAP_START, MMAP_START),
            Ok(HEAP_INITIAL_END)
        );
        assert_eq!(
            heap_mapping_end(HEAP_INITIAL_END + 1, MMAP_START),
            Ok(HEAP_INITIAL_END + 4096)
        );
        assert_eq!(heap_mapping_end(MMAP_START, MMAP_START), Ok(MMAP_START));
        assert_eq!(heap_mapping_end(MMAP_START + 1, MMAP_START), Err(ENOMEM));
    }

    #[test]
    fn validates_anonymous_mapping_user_address_range() {
        assert_eq!(validate_mapping_range(0x10000, 4096), Ok(()));
        assert_eq!(validate_mapping_range(MMAP_END - 4096, 4096), Ok(()));
        assert_eq!(validate_mapping_range(0xFFFF, 4096), Err(EINVAL));
        assert_eq!(validate_mapping_range(MMAP_END - 4095, 4096), Err(EINVAL));
    }

    #[test]
    fn resolves_process_relative_paths_without_escaping_vfs_root() {
        assert_eq!(
            resolve_user_path("/home/user", "../shared/data").unwrap(),
            "/home/shared/data"
        );
        assert_eq!(
            resolve_user_path("/home/user", "/etc/config").unwrap(),
            "/etc/config"
        );
        assert_eq!(
            resolve_user_path("/", "../../outside"),
            Err(FsError::InvalidPath)
        );
        assert_eq!(resolve_user_path("/", ""), Err(FsError::InvalidPath));
    }

    #[test]
    fn reports_devfs_tty_nodes_as_character_devices() {
        assert_eq!(stat_mode("/dev/tty", NodeType::File, 0o666), 0o020666);
        assert_eq!(stat_mode("/tmp/file", NodeType::File, 0o644), 0o100644);
    }

    #[test]
    fn builds_an_aligned_initial_stack_with_argv_and_empty_envp() {
        let arguments = alloc::vec![String::from("program"), String::from("two words")];
        let (stack_pointer, stack) = build_initial_stack(0x7000, 4096, &arguments, &[]).unwrap();
        let word =
            |offset: usize| u64::from_le_bytes(stack[offset..offset + 8].try_into().unwrap());
        assert_eq!(stack_pointer % 16, 0);
        assert_eq!(word(0), 2);

        let first = (word(8) - stack_pointer) as usize;
        let second = (word(16) - stack_pointer) as usize;
        assert_eq!(&stack[first..first + 8], b"program\0");
        assert_eq!(&stack[second..second + 10], b"two words\0");
        assert_eq!(word(24), 0);
        assert_eq!(word(32), 0);
    }

    #[test]
    fn builds_initial_stack_with_environment_pointers() {
        let arguments = alloc::vec![String::from("program")];
        let environment = alloc::vec![String::from("PATH=/bin:/usr/bin"), String::from("HOME=/"),];
        let (stack_pointer, stack) =
            build_initial_stack(0x7000, 4096, &arguments, &environment).unwrap();
        let word =
            |offset: usize| u64::from_le_bytes(stack[offset..offset + 8].try_into().unwrap());
        assert_eq!(word(0), 1);
        assert_eq!(word(16), 0);

        let path = (word(24) - stack_pointer) as usize;
        let home = (word(32) - stack_pointer) as usize;
        assert_eq!(&stack[path..path + 19], b"PATH=/bin:/usr/bin\0");
        assert_eq!(&stack[home..home + 7], b"HOME=/\0");
        assert_eq!(word(40), 0);
    }

    #[test]
    fn rejects_invalid_environment_entries() {
        let arguments = alloc::vec![String::from("program")];
        let invalid = alloc::vec![String::from("=value")];
        assert_eq!(
            build_initial_stack(0x7000, 4096, &arguments, &invalid),
            Err(ElfError::InvalidArguments)
        );
    }

    #[test]
    fn scheduler_selects_runnable_processes_in_round_robin_order() {
        let runnable = [true, true, false];
        assert_eq!(
            next_runnable(runnable.len(), 0, |index| runnable[index]),
            Some(1)
        );
        assert_eq!(
            next_runnable(runnable.len(), 1, |index| runnable[index]),
            Some(0)
        );
        let runnable = [true, false, true];
        assert_eq!(
            next_runnable(runnable.len(), 0, |index| runnable[index]),
            Some(2)
        );
        assert_eq!(next_runnable(1, 0, |_| true), Some(0));
        assert_eq!(next_runnable(0, 0, |_| true), None);
        assert_eq!(next_runnable(2, 2, |_| true), None);
    }

    #[test]
    fn scheduler_returns_none_when_every_process_has_exited() {
        let runnable = [false, false, false];
        assert_eq!(
            next_runnable(runnable.len(), 1, |index| runnable[index]),
            None
        );
    }

    #[test]
    fn wait_target_matches_specific_or_any_child() {
        assert!(wait_target_matches(7, 7));
        assert!(!wait_target_matches(7, 8));
        assert!(wait_target_matches(WAIT_ANY_PID, 7));
    }

    #[test]
    fn pipe_handles_empty_eof_capacity_and_wrapped_data() {
        let mut buffer = Vec::new();
        buffer.resize(PIPE_CAPACITY, 0);
        let mut pipe = Pipe {
            id: 1,
            buffer,
            head: 0,
            length: 0,
            readers: 1,
            writers: 1,
        };
        let mut output = [0; 5];
        assert_eq!(pipe.read(&mut output), Err(EAGAIN));

        assert_eq!(pipe.write(b"abc"), Ok(3));
        assert_eq!(pipe.read(&mut output[..2]), Ok(2));
        assert_eq!(&output[..2], b"ab");
        assert_eq!(pipe.write(b"defg"), Ok(4));
        assert_eq!(pipe.read(&mut output), Ok(5));
        assert_eq!(&output, b"cdefg");

        let full = [1; PIPE_CAPACITY];
        assert_eq!(pipe.write(&full), Ok(PIPE_CAPACITY));
        assert_eq!(pipe.write(b"x"), Err(EAGAIN));
        pipe.readers = 0;
        assert_eq!(pipe.write(b"x"), Err(EPIPE));
        pipe.writers = 0;
        let mut drain = [0; PIPE_CAPACITY];
        assert_eq!(pipe.read(&mut drain), Ok(PIPE_CAPACITY));
        assert_eq!(pipe.read(&mut drain), Ok(0));
    }

    #[test]
    fn pipe_endpoints_receive_distinct_descriptor_slots() {
        let mut descriptors = Vec::new();
        descriptors.extend([
            Some(UserDescriptor::ConsoleInput),
            Some(UserDescriptor::ConsoleOutput),
            Some(UserDescriptor::ConsoleError),
        ]);
        let (read_end, write_end) = create_pipe().unwrap();

        assert_eq!(
            install_pipe_descriptors(&mut descriptors, read_end, write_end),
            Ok((3, 4))
        );
        assert!(descriptors[3].as_ref().is_some_and(|descriptor| matches!(
            descriptor,
            UserDescriptor::Pipe(endpoint) if endpoint.readable && !endpoint.writable
        )));
        assert!(descriptors[4].as_ref().is_some_and(|descriptor| matches!(
            descriptor,
            UserDescriptor::Pipe(endpoint) if !endpoint.readable && endpoint.writable
        )));
    }

    #[test]
    fn rejects_arguments_with_nul_or_exceeding_limits() {
        let nul_argument = alloc::vec![String::from("bad\0argument")];
        assert_eq!(
            build_initial_stack(0x7000, 4096, &nul_argument, &[]),
            Err(ElfError::InvalidArguments)
        );

        let too_many = alloc::vec![String::from("arg"); 65];
        assert_eq!(
            build_initial_stack(0x7000, 4096, &too_many, &[]),
            Err(ElfError::ArgumentsTooLarge)
        );
    }
}
