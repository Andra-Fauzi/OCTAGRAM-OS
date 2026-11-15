#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

use core::arch::asm;

use limine::BaseRevision;
use limine::request::{RequestsEndMarker, RequestsStartMarker};

/// Sets the base revision to the latest revision supported by the crate.
/// See specification for further info.
/// Be sure to mark all limine requests with #[used], otherwise they may be removed by the compiler.
#[used]
// The .requests section allows limine to find the requests faster and more safely.
#[unsafe(link_section = ".requests")]
static BASE_REVISION: BaseRevision = BaseRevision::new();

/// Define the stand and end markers for Limine requests.
#[used]
#[unsafe(link_section = ".requests_start_marker")]
static _START_MARKER: RequestsStartMarker = RequestsStartMarker::new();
#[used]
#[unsafe(link_section = ".requests_end_marker")]
static _END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

mod arch;
mod drivers;
pub mod elf;
pub mod fs;
mod graphics;
mod memory;
pub mod network;
mod rsdp;
mod shell;
pub mod storage;
pub mod task;
mod terminal;
pub mod toolchain;

extern crate alloc;

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    // All limine requests must also be referenced in a called function, otherwise they may be
    // removed by the linker.
    assert!(BASE_REVISION.is_supported());
    drivers::early_init();

    if let Some(framebuffer) = graphics::Framebuffer::get() {
        for i in 0..100_u64 {
            let pixel_offset = i * framebuffer.pitch() + i * 4;
            unsafe {
                framebuffer
                    .addr()
                    .add(pixel_offset as usize)
                    .cast::<u32>()
                    .write(0xFFFFFFFF)
            };
        }
    }
    unsafe { asm!("cli") };

    unsafe { arch::x86_64::fpu::init() };
    unsafe { arch::x86_64::gdt::load_gdt() };
    println!("GDT LOADED");
    unsafe { arch::x86_64::idt::load_idt() };
    println!("IDT LOADED");
    rsdp::test_rsdp();
    arch::x86_64::apic::disable_pic();
    arch::x86_64::apic::LAPIC.call_once(|| unsafe { arch::x86_64::apic::init_lapic() });
    unsafe { arch::x86_64::apic::init_ioapic() };

    unsafe { asm!("sti") };
    memory::init();
    fs::init();
    unsafe { drivers::initialize() };

    let lapic = arch::x86_64::apic::LAPIC
        .get()
        .expect("LAPIC must be initialized before the scheduler timer");
    unsafe {
        lapic.start_periodic_timer(arch::x86_64::interrupt::TIMER_VECTOR, 1_000_000);
    }

    task::spawn(async {
        println!("Cooperative task executor is ready");
    });
    for terminal in 0..terminal::TERMINAL_COUNT {
        let shell_handle = task::spawn(shell::run(terminal));
        drivers::keyboard::set_input_waker(terminal, shell_handle);
    }
    hcf();
}

#[cfg(not(test))]
#[panic_handler]
fn rust_panic(_info: &core::panic::PanicInfo) -> ! {
    print!("\n{:#?}\n", _info);
    hcf();
}

fn hcf() -> ! {
    loop {
        if !task::run_ready() {
            unsafe {
                #[cfg(target_arch = "x86_64")]
                {
                    asm!("cli");
                    if task::has_ready_tasks() {
                        asm!("sti");
                    } else {
                        asm!("sti; hlt");
                    }
                }
                #[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
                if !task::has_ready_tasks() {
                    asm!("wfi");
                }
                #[cfg(target_arch = "loongarch64")]
                if !task::has_ready_tasks() {
                    asm!("idle 0");
                }
            }
        }

        // Deferred driver work runs after interrupt completion.
        unsafe { drivers::poll_deferred() };
        crate::network::poll_tcp();
    }
}
