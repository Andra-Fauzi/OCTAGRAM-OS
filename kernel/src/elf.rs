//! Loader for static ELF64 x86_64 programs and the minimal OCTAGRAM syscall ABI.

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::arch::x86_64::{
    gdt,
    interrupt::{self, InterruptFrame, InterruptStackFrame},
};
use crate::fs::{self, FsError, NodeType};
use crate::memory::{
    allocate_frame, free_frame, paging,
    paging::{PAGE_SIZE, PagingError},
};

const ELF_HEADER_SIZE: usize = 64;
const PROGRAM_HEADER_SIZE: usize = 56;
const MAX_PROGRAM_HEADERS: usize = 128;
const MAX_MAPPED_PAGES: u64 = 4096;
const STACK_PAGES: u64 = 8;
const MAX_ARGUMENTS: usize = 64;
const MAX_ARGUMENT_BYTES: usize = 4096;
const STACK_TOP: u64 = paging::USER_ADDRESS_MAX;
const HEAP_START: u64 = 0x1000_0000;
const HEAP_PAGES: u64 = 256;
const MAX_USER_PATH: usize = 256;
const MAX_SYSCALL_IO: usize = 4096;
const MAX_OPEN_FILES: usize = 16;
const MAX_SCHEDULED_PROCESSES: usize = 16;
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

const O_RDONLY: u64 = 0;
const O_WRONLY: u64 = 1;
const O_RDWR: u64 = 2;
const O_ACCESS_MASK: u64 = 3;
const O_CREAT: u64 = 1 << 8;
const O_TRUNC: u64 = 1 << 9;
const O_APPEND: u64 = 1 << 10;

const EINTR: i32 = 4;
const EIO: i32 = 5;
const EBADF: i32 = 9;
const ENOMEM: i32 = 12;
const EFAULT: i32 = 14;
const ENOENT: i32 = 2;
const EEXIST: i32 = 17;
const ENOTDIR: i32 = 20;
const EISDIR: i32 = 21;
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
    heap_end: u64,
    mapped_pages: Vec<u64>,
    address_space: paging::AddressSpace,
}

struct OpenFile {
    path: String,
    offset: usize,
    readable: bool,
    writable: bool,
    append: bool,
}

enum UserDescriptor {
    ConsoleInput,
    ConsoleOutput,
    ConsoleError,
    File(OpenFile),
}

struct UserProcess {
    active: bool,
    pid: u64,
    descriptors: Vec<Option<UserDescriptor>>,
    cwd: String,
    program_break: u64,
    heap_end: u64,
}

impl UserProcess {
    const fn new() -> Self {
        Self {
            active: false,
            pid: 0,
            descriptors: Vec::new(),
            cwd: String::new(),
            program_break: HEAP_START,
            heap_end: HEAP_START,
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
    _kernel_stack: Option<ProcessKernelStack>,
    kernel_stack_top: u64,
    frame_pointer: usize,
    exit_status: Option<u64>,
}

struct UserScheduler {
    processes: Vec<ScheduledProcess>,
    current: usize,
    active: bool,
}

impl UserScheduler {
    const fn new() -> Self {
        Self {
            processes: Vec::new(),
            current: 0,
            active: false,
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

impl LoadedElf {
    /// Run one trusted program until it exits or faults, then release its pages.
    pub fn run(self) -> Result<u64, ElfError> {
        self.run_with_cwd("/")
    }

    /// Run one trusted program with a process-local current working directory.
    pub fn run_with_cwd(self, cwd: &str) -> Result<u64, ElfError> {
        let mut images = Vec::new();
        if images.try_reserve_exact(1).is_err() {
            self.unload()?;
            return Err(ElfError::OutOfMemory);
        }
        images.push(self);
        let statuses = run_loaded_processes(images, cwd)?;
        statuses
            .first()
            .copied()
            .ok_or(ElfError::SchedulerInvariant)
    }

    /// Release the mappings without running the program.
    pub fn unload(mut self) -> Result<(), ElfError> {
        unsafe { self.address_space.activate() };
        let page_cleanup = release_pages(&mut self.mapped_pages);
        let table_cleanup = self.address_space.destroy();
        page_cleanup?;
        table_cleanup?;
        Ok(())
    }
}

pub fn load(bytes: &[u8]) -> Result<LoadedElf, ElfError> {
    load_with_args(bytes, &[])
}

pub fn load_with_args(bytes: &[u8], arguments: &[String]) -> Result<LoadedElf, ElfError> {
    let (entry, segments) = parse(bytes)?;
    let heap_end = HEAP_START
        .checked_add(HEAP_PAGES * PAGE_SIZE)
        .ok_or(ElfError::AddressRange)?;
    let stack_start = STACK_TOP
        .checked_sub(STACK_PAGES * PAGE_SIZE)
        .ok_or(ElfError::AddressRange)?;
    let guard_start = stack_start
        .checked_sub(PAGE_SIZE)
        .ok_or(ElfError::AddressRange)?;

    let mut total_pages = STACK_PAGES + HEAP_PAGES;
    for (index, segment) in segments.iter().enumerate() {
        let start = page_start(segment.virtual_address);
        let end = page_end(segment.virtual_address, segment.memory_size)?;
        if (start < STACK_TOP && guard_start < end) || (start < heap_end && HEAP_START < end) {
            return Err(ElfError::AddressRange);
        }
        total_pages = total_pages
            .checked_add((end - start) / PAGE_SIZE)
            .ok_or(ElfError::TooLarge)?;
        if total_pages > MAX_MAPPED_PAGES {
            return Err(ElfError::TooLarge);
        }
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
        .try_reserve_exact(total_pages as usize)
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
            build_initial_stack(stack_start, STACK_PAGES * PAGE_SIZE, arguments)?;
        copy_to_user_pages(user_stack, &initial_stack)?;
        Ok::<u64, ElfError>(user_stack)
    })();

    match load_result {
        Ok(user_stack) => {
            unsafe { address_space.activate_kernel() };
            Ok(LoadedElf {
                entry,
                user_stack,
                heap_end,
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
    let bytes = read_executable_file(path)?;
    load_with_args(&bytes, arguments)?.run_with_cwd(cwd)
}

/// Run several ELF files concurrently under the user-process scheduler.
pub fn run_files_parallel_from(paths: &[String], cwd: &str) -> Result<Vec<u64>, ElfError> {
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
        match load_with_args(&bytes, core::slice::from_ref(path)) {
            Ok(image) => images.push(image),
            Err(error) => return fail_after_unload(images, error),
        }
    }

    run_loaded_processes(images, cwd)
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

fn run_loaded_processes(images: Vec<LoadedElf>, cwd: &str) -> Result<Vec<u64>, ElfError> {
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
        match new_user_process(image.heap_end, cwd, pid) {
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
    for ((image, state), kernel_stack) in images.into_iter().zip(states).zip(stacks) {
        let kernel_stack_top = kernel_stack
            .as_ref()
            .map_or_else(gdt::user_kernel_stack_top, ProcessKernelStack::top);
        let frame_pointer = kernel_stack
            .as_ref()
            .map_or(0, |stack| initialize_user_frame(&image, stack.top()));
        processes.push(ScheduledProcess {
            image,
            state,
            _kernel_stack: kernel_stack,
            kernel_stack_top,
            frame_pointer,
            exit_status: None,
        });
    }

    {
        let mut scheduler = USER_SCHEDULER.lock();
        scheduler.processes = processes;
        scheduler.current = 0;
        scheduler.active = true;
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
    let _last_status = unsafe { gdt::enter_user_mode(entry, user_stack) };

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
    for process in processes {
        if let Some(status) = process.exit_status {
            if !status_reserve_failed {
                statuses.push(status);
            }
        } else {
            missing_status = true;
        }
        if let Err(error) = process.image.unload()
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
) -> Result<(u64, Vec<u8>), ElfError> {
    if arguments.len() > MAX_ARGUMENTS {
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

    let table_words = arguments
        .len()
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
    let mut initial_stack = Vec::new();
    initial_stack
        .try_reserve_exact(stack_size - cursor)
        .map_err(|_| ElfError::OutOfMemory)?;
    initial_stack.extend_from_slice(&stack[cursor..]);
    Ok((stack_pointer, initial_stack))
}

pub fn handle_syscall(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    match frame.rax {
        SYSCALL_EXIT => terminate_user(frame, frame.rdi),
        number => {
            frame.rax = match dispatch_syscall(number, frame.rdi, frame.rsi, frame.rdx) {
                Ok(value) => value,
                Err(errno) => (-(errno as i64)) as u64,
            };
            frame_pointer
        }
    }
}

pub fn handle_timer(frame: &mut InterruptFrame) -> *mut InterruptFrame {
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
        scheduler.processes[index].exit_status.is_none()
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

fn dispatch_syscall(number: u64, arg1: u64, arg2: u64, arg3: u64) -> Result<u64, i32> {
    match number {
        SYSCALL_WRITE => syscall_write(arg1, arg2, arg3),
        SYSCALL_READ => syscall_read(arg1, arg2, arg3),
        SYSCALL_OPEN => syscall_open(arg1, arg2),
        SYSCALL_CLOSE => syscall_close(arg1),
        SYSCALL_LSEEK => syscall_lseek(arg1, arg2 as i64, arg3),
        SYSCALL_STAT => syscall_stat(arg1, arg2),
        SYSCALL_MKDIR => syscall_mkdir(arg1),
        SYSCALL_UNLINK => syscall_unlink(arg1),
        SYSCALL_FTRUNCATE => syscall_ftruncate(arg1, arg2),
        SYSCALL_BRK => syscall_brk(arg1),
        SYSCALL_CHDIR => syscall_chdir(arg1),
        SYSCALL_GETCWD => syscall_getcwd(arg1, arg2),
        SYSCALL_GETPID => {
            let process = USER_PROCESS.lock();
            if process.active {
                Ok(process.pid)
            } else {
                Err(EINTR)
            }
        }
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
                None => Err(EBADF),
            }
        }
        _ => Err(ENOSYS),
    }
}

fn new_user_process(heap_end: u64, cwd: &str, pid: u64) -> Result<UserProcess, ElfError> {
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
        descriptors,
        cwd: process_cwd,
        program_break: HEAP_START,
        heap_end,
    })
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
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let descriptor = process
        .descriptors
        .get_mut(fd)
        .and_then(Option::as_mut)
        .ok_or(EBADF)?;
    match descriptor {
        UserDescriptor::ConsoleOutput => fs::with_vfs(|vfs| vfs.write("/dev/stdout", 0, bytes))
            .map_err(errno_from_fs)?
            .map(|count| count as u64)
            .map_err(errno_from_fs),
        UserDescriptor::ConsoleError => fs::with_vfs(|vfs| vfs.write("/dev/stderr", 0, bytes))
            .map_err(errno_from_fs)?
            .map(|count| count as u64)
            .map_err(errno_from_fs),
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
        UserDescriptor::ConsoleInput => Err(EBADF),
    }
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
    let descriptor = process
        .descriptors
        .get_mut(fd)
        .and_then(Option::as_mut)
        .ok_or(EBADF)?;
    match descriptor {
        UserDescriptor::ConsoleInput => fs::with_vfs(|vfs| vfs.read("/dev/stdin", 0, output))
            .map_err(errno_from_fs)?
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
        UserDescriptor::ConsoleOutput | UserDescriptor::ConsoleError => Err(EBADF),
    }
}

fn syscall_open(address: u64, flags: u64) -> Result<u64, i32> {
    const ALLOWED_FLAGS: u64 = O_ACCESS_MASK | O_CREAT | O_TRUNC | O_APPEND;
    if flags & !ALLOWED_FLAGS != 0 || flags & O_ACCESS_MASK == 3 {
        return Err(EINVAL);
    }
    let writable = flags & O_ACCESS_MASK != O_RDONLY;
    let readable = flags & O_ACCESS_MASK != O_WRONLY;
    if flags & O_TRUNC != 0 && !writable || flags & O_APPEND != 0 && !writable {
        return Err(EINVAL);
    }
    let path = resolve_process_path(&copy_user_path(address)?)?;
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
        let metadata = match vfs.metadata(&path) {
            Ok(metadata) => metadata,
            Err(FsError::NotFound) if flags & O_CREAT != 0 => {
                vfs.create_file(&path)?;
                vfs.metadata(&path)?
            }
            Err(error) => return Err(error),
        };
        if metadata.node_type == NodeType::Directory {
            return Err(FsError::IsDirectory);
        }
        if (readable && metadata.permissions & 0o400 == 0)
            || (writable && metadata.permissions & 0o200 == 0)
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
    *descriptor = None;
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
    if !(HEAP_START..=process.heap_end).contains(&address) {
        return Err(ENOMEM);
    }
    process.program_break = address;
    Ok(address)
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
    if address == 0 {
        return Err(EFAULT);
    }
    let mut bytes = Vec::new();
    bytes.try_reserve(MAX_USER_PATH).map_err(|_| ENOMEM)?;
    for index in 0..MAX_USER_PATH {
        let current = address.checked_add(index as u64).ok_or(EFAULT)?;
        paging::validate_user_range(current, 1, false).map_err(|_| EFAULT)?;
        let byte = unsafe { (current as *const u8).read() };
        if byte == 0 {
            return String::from_utf8(bytes).map_err(|_| EILSEQ);
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

pub fn terminate_user(frame: &mut InterruptFrame, status: u64) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    if frame.stack_frame.code_segment & 3 != 3 {
        return frame_pointer;
    }

    let mut scheduler = USER_SCHEDULER.lock();
    if scheduler.active && !scheduler.processes.is_empty() {
        let current = scheduler.current;
        if current < scheduler.processes.len() {
            scheduler.processes[current].exit_status = Some(status);
            let mut active_process = USER_PROCESS.lock();
            core::mem::swap(
                &mut *active_process,
                &mut scheduler.processes[current].state,
            );
            scheduler.processes[current].state.active = false;
            if let Some(next) = next_runnable(scheduler.processes.len(), current, |index| {
                scheduler.processes[index].exit_status.is_none()
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
        EACCES, EEXIST, EILSEQ, ENOENT, ENOSYS, ElfError, NodeType, build_initial_stack,
        dispatch_syscall, errno_from_fs, next_runnable, parse, resolve_user_path, stat_mode,
    };
    use crate::fs::FsError;

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
        assert_eq!(dispatch_syscall(u64::MAX, 0, 0, 0), Err(ENOSYS));
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
        let (stack_pointer, stack) = build_initial_stack(0x7000, 4096, &arguments).unwrap();
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
    fn rejects_arguments_with_nul_or_exceeding_limits() {
        let nul_argument = alloc::vec![String::from("bad\0argument")];
        assert_eq!(
            build_initial_stack(0x7000, 4096, &nul_argument),
            Err(ElfError::InvalidArguments)
        );

        let too_many = alloc::vec![String::from("arg"); 65];
        assert_eq!(
            build_initial_stack(0x7000, 4096, &too_many),
            Err(ElfError::ArgumentsTooLarge)
        );
    }
}
