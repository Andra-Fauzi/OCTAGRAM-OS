//! Enumerasi device USB, dispatch ke device-class driver HID, dan
//! hotplug (connect/disconnect) berbasis diffing port state.
//!
//! Modul ini yang "tahu" alur enumerasi USB standar di atas transport
//! xHCI generic (lihat `xhci.rs`): scan port yang connected, enable
//! slot, address device, ambil descriptor, cari endpoint interrupt IN,
//! lalu dispatch ke driver yang sesuai (`keyboard_usb` / `mouse_usb`)
//! berdasarkan `bInterfaceProtocol`. Tidak ada satu pun device-class
//! (keyboard, mouse, dst) yang di-hardcode di sini -- generic terhadap
//! jumlah & jenis device HID yang tercolok.
//!
//! # Hotplug
//! `ACTIVE_PORTS` menyimpan port mana saja yang SUDAH punya slot xHCI.
//! `discover_and_init_hid_devices()` men-diff daftar itu terhadap port
//! yang connected SEKARANG:
//! - Port yang dulu aktif tapi sekarang sudah tidak connected -> di-
//!   *disconnect*: bersihin state driver (`keyboard_usb`/`mouse_usb`)
//!   lalu `xhci::disable_slot`.
//! - Port yang connected tapi belum ada di `ACTIVE_PORTS` -> di-*init*
//!   seperti device baru.
//! Port yang statusnya tidak berubah dilewati sama sekali -- makanya
//! fungsi ini AMAN dipanggil berkali-kali (idempotent), tidak akan
//! nge-re-enumerasi device yang sudah jalan.
//!
//! PENTING: jangan panggil `discover_and_init_hid_devices()` (atau
//! `poll_hotplug()`) dari DALAM interrupt handler vector 44. Proses
//! connect (`enable_slot`/`address_device`/dst) itu `cli`+kirim command
//! lalu SPIN nunggu Command Completion Event, dan completion event itu
//! sendiri baru bisa nyampe lewat interrupt vector 44 berikutnya --
//! yang tidak akan pernah nembak selama kita masih di dalam ISR vector
//! 44 yang sekarang (EOI-nya belum dikirim). Makanya alurnya: event
//! ring cuma nyalain flag `xhci::PORT_CHANGE_PENDING`, lalu idle loop
//! (`hcf` di `main.rs`, jalan SETELAH ISR selesai & EOI terkirim) yang
//! manggil `poll_hotplug()`.

pub mod keyboard_usb;
pub mod mouse_usb;
pub mod xhci;

use crate::pci;

use crate::println;
use core::sync::atomic::Ordering;
use spin::Mutex;
use xhci::XHCI;

/// Jenis device HID yang berhasil di-dispatch ke satu endpoint, dipakai
/// buat nyatet driver mana yang perlu di-shutdown kalau slot ini nanti
/// disconnect.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DeviceKind {
    Keyboard,
    Mouse,
}

/// Satu entry "device ini lagi aktif di root port ini, dengan slot ID
/// ini". `has_keyboard`/`has_mouse` dicatat per-slot karena satu device
/// composite (mis. keyboard dengan trackpoint) bisa punya lebih dari
/// satu interface HID di slot yang sama.
struct ActivePort {
    root_port: u8,
    slot_id: u32,
    has_keyboard: bool,
    has_mouse: bool,
}

/// Registry port yang lagi aktif (sudah enable_slot + address_device
/// sukses). Sumber kebenaran buat diffing di `discover_and_init_hid_devices`.
static ACTIVE_PORTS: Mutex<alloc::vec::Vec<ActivePort>> = Mutex::new(alloc::vec::Vec::new());

/// Cari semua USB host controller di PCI bus. Untuk yang xHCI (prog_if
/// 0x30): enable device, baca BAR0, setup MSI-X, init controller, scan
/// port, lalu -- kalau MSI-X berhasil -- taruh instance-nya ke
/// `xhci::XHCI_INSTANCE` dan lanjut enumerasi device HID di atasnya.
/// Controller non-xHCI (UHCI/OHCI/EHCI) cuma dilaporkan, belum ada
/// driver-nya.
///
/// Kalau MSI-X gagal, fallback ke polling manual (loop tanpa akhir) --
/// PERHATIAN: ini artinya controller berikutnya di `usb_controllers`
/// (kalau ada) tidak akan pernah diproses, sama seperti perilaku lama
/// di `main.rs`.
pub unsafe fn init_usb_controllers() {
    unsafe {
        let usb_controllers = pci::find_usb_controllers();
        for ctrl in &usb_controllers {
            let kind = match ctrl.prog_if {
                0x00 => "UHCI",
                0x10 => "OHCI",
                0x20 => "EHCI",
                0x30 => "xHCI",
                _ => "Unknown",
            };
            println!(
                "USB Controller found: {} at {:02x}:{:02x}.{}",
                kind, ctrl.bus, ctrl.device, ctrl.function
            );

            if ctrl.prog_if == 0x30 {
                init_xhci_controller(ctrl);
            }
        }
    }
}

/// Setup satu controller xHCI: enable & baca BAR0, setup MSI-X, init
/// controller (DCBAA/Command Ring/Event Ring), scan port, lalu -- kalau
/// interrupt-driven mode (MSI-X) aktif -- publish instance-nya dan mulai
/// enumerasi device HID. Kalau MSI-X gagal, fallback ke polling manual
/// yang tidak pernah return (dipakai sebagai loop utama kernel).
unsafe fn init_xhci_controller(ctrl: &pci::PciDevice) {
    unsafe {
        pci::enable_device(ctrl);
        let bar = pci::get_bar0(ctrl);
        println!(
            "xHCI BAR0: phys={:#x} 64bit={} prefetchable={}",
            bar.address, bar.is_64bit, bar.is_prefetchable
        );

        let mut xhci_dev = XHCI::new(bar, ctrl);

        let msix_ok = xhci_dev.setup_msix(ctrl, 44, 0);

        //pci::enable_msi(ctrl, 44, 0);
        xhci::init_xhci(&mut xhci_dev);
        xhci::scan_ports(&xhci_dev);

        if msix_ok {
            xhci::XHCI_INSTANCE.call_once(|| spin::Mutex::new(xhci_dev));
            println!("Interrupt-driven mode aktif (MSI-X)");

            // Generic: enumerasi semua port + dispatch driver
            // (keyboard, mouse, atau HID lain) otomatis. Registry
            // ACTIVE_PORTS masih kosong di titik ini, jadi semua port
            // connected dianggap "baru" -- perilakunya sama seperti
            // full scan biasa.
            discover_and_init_hid_devices();
        } else {
            println!("Fallback ke polling manual...");
            loop {
                xhci_dev.poll_event_ring();
            }
        }
    }
}

/// Dipanggil dari idle loop (`hcf`) setelah bangun dari `hlt`. Kalau
/// ada Port Status Change Event yang masuk sejak terakhir dicek
/// (`xhci::PORT_CHANGE_PENDING`), jalankan sinkronisasi hotplug penuh.
/// Aman dipanggil terus-menerus -- kalau flag-nya belum nyala, fungsi
/// ini langsung return tanpa nyentuh xHCI sama sekali.
pub unsafe fn poll_hotplug() {
    unsafe {
        if !xhci::PORT_CHANGE_PENDING.swap(false, Ordering::SeqCst) {
            return;
        }
        // Kalau instance xHCI belum ke-init (harusnya tidak mungkin di
        // titik ini, tapi jaga-jaga), jangan sentuh apa pun.
        if xhci::XHCI_INSTANCE.get().is_none() {
            return;
        }
        println!("Port Status Change terdeteksi, sinkronisasi hotplug...");
        discover_and_init_hid_devices();
    }
}

/// Sinkronisasi `ACTIVE_PORTS` terhadap kondisi port SEKARANG:
/// - Port yang dulu di `ACTIVE_PORTS` tapi sekarang sudah tidak
///   connected -> disconnect (bersihin driver + disable slot).
/// - Port yang connected tapi belum ada di `ACTIVE_PORTS` -> enumerasi
///   & dispatch seperti device baru.
///
/// Idempotent: dipanggil pertama kali waktu boot (registry kosong, jadi
/// semua port connected dianggap baru) maupun berkali-kali lagi tiap
/// ada hotplug -- port yang statusnya tidak berubah tidak disentuh.
pub unsafe fn discover_and_init_hid_devices() {
    unsafe {
        // Ambil daftar port yang connected dulu dalam scope lock pendek,
        // supaya tidak deadlock dengan fungsi-fungsi enumerate di bawah
        // (enable_slot/address_device/dst juga mengunci XHCI_INSTANCE
        // sendiri-sendiri).
        let port_states: alloc::vec::Vec<(u8, u8)> = {
            let guard = xhci::XHCI_INSTANCE.get().unwrap().lock();
            let cap = guard.cap_regs();
            let op = guard.op_regs();
            let max_ports = cap.max_ports();

            let mut states = alloc::vec::Vec::new();
            for port in 0..max_ports {
                let portsc = op.read_portsc(port);
                let connected = portsc & 0x1 != 0;
                if connected {
                    let speed = ((portsc >> 10) & 0xF) as u8;
                    states.push((port, speed));
                }
            }
            states
        };

        println!(
            "HID discovery: {} port terhubung ditemukan",
            port_states.len()
        );

        let mut active = ACTIVE_PORTS.lock();

        // --- DISCONNECT: port yang dulu aktif, sekarang sudah tidak connected ---
        let mut i = 0;
        while i < active.len() {
            let root_port = active[i].root_port;
            let still_connected = port_states
                .iter()
                .any(|&(port_index, _)| port_index + 1 == root_port);

            if still_connected {
                i += 1;
                continue;
            }

            // remove() disini aman -- ACTIVE_PORTS biasanya kecil (jumlah
            // device HID yang tercolok), jadi shift-nya murah.
            let entry = active.remove(i);
            shutdown_device(&entry);
            // Jangan i += 1 -- index i sekarang nunjuk elemen berikutnya
            // (kalau ada) setelah remove.
        }

        // --- CONNECT: port yang connected tapi belum ada di registry ---
        for &(port_index, speed) in &port_states {
            let root_port = port_index + 1;
            let already_active = active.iter().any(|e| e.root_port == root_port);
            if already_active {
                continue;
            }

            if let Some(entry) = init_one_device(root_port, speed) {
                active.push(entry);
            }
        }
    }
}

/// Bersihin satu device yang barusan disconnect: shutdown driver
/// (keyboard/mouse, kalau ada) lalu kirim Disable Slot ke controller.
unsafe fn shutdown_device(entry: &ActivePort) {
    unsafe {
        println!(
            "--- Device di root port {} (slot={}) disconnect ---",
            entry.root_port, entry.slot_id
        );

        if entry.has_keyboard {
            keyboard_usb::shutdown_keyboard(entry.slot_id);
        }
        if entry.has_mouse {
            mouse_usb::shutdown_mouse(entry.slot_id);
        }

        if let Err(e) = xhci::disable_slot(entry.slot_id) {
            println!("  disable_slot gagal: {}", e);
        }
    }
}

/// Enumerasi & konfigurasi satu device baru di satu root port, lalu
/// dispatch semua endpoint interrupt IN-nya ke driver HID yang sesuai.
/// Return `Some(ActivePort)` kalau slot berhasil dibuat (dicatat ke
/// registry oleh caller) walaupun tidak ada satu pun endpoint HID yang
/// akhirnya jalan (mis. device-nya bukan HID) -- ini supaya port itu
/// tidak dicoba enumerasi ulang terus-menerus tiap hotplug event
/// selanjutnya. Return `None` kalau bahkan `enable_slot`/`address_device`
/// gagal (device dianggap belum "aktif", boleh dicoba lagi nanti).
unsafe fn init_one_device(root_port: u8, speed: u8) -> Option<ActivePort> {
    unsafe {
        println!(
            "--- Enumerasi device di root port {} (speed={}) ---",
            root_port, speed
        );

        let slot_id = match xhci::enable_slot() {
            Ok(id) => id,
            Err(e) => {
                println!("  enable_slot gagal: {}", e);
                return None;
            }
        };

        if let Err(e) = xhci::address_device(slot_id, root_port, speed) {
            println!("  address_device gagal: {}", e);
            return None;
        }

        if let Err(e) = xhci::get_device_descriptor(slot_id) {
            println!("  get_device_descriptor gagal: {}", e);
            // Slot sudah ke-address, catat sebagai aktif (tanpa HID)
            // supaya tidak dicoba di-enumerasi ulang tiap hotplug event.
            return Some(ActivePort {
                root_port,
                slot_id,
                has_keyboard: false,
                has_mouse: false,
            });
        }

        let endpoints =
            match xhci::get_configuration_descriptor_and_find_interrupt_endpoints(slot_id) {
                Ok(eps) => eps,
                Err(e) => {
                    println!("  tidak ada endpoint interrupt IN: {}", e);
                    return Some(ActivePort {
                        root_port,
                        slot_id,
                        has_keyboard: false,
                        has_mouse: false,
                    });
                }
            };

        // SET_CONFIGURATION 1 -- wajib sebelum Configure Endpoint command
        if let Err(e) = xhci::control_transfer_no_data(slot_id, 0x00, 0x09, 1, 0) {
            println!("  SET_CONFIGURATION gagal: {}", e);
            return Some(ActivePort {
                root_port,
                slot_id,
                has_keyboard: false,
                has_mouse: false,
            });
        }

        let mut has_keyboard = false;
        let mut has_mouse = false;
        for ep_info in endpoints {
            match dispatch_endpoint(slot_id, root_port, ep_info) {
                Some(DeviceKind::Keyboard) => has_keyboard = true,
                Some(DeviceKind::Mouse) => has_mouse = true,
                None => {}
            }
        }

        Some(ActivePort {
            root_port,
            slot_id,
            has_keyboard,
            has_mouse,
        })
    }
}

/// Konfigurasi satu endpoint interrupt IN, lalu serahkan ke device-class
/// driver yang tepat berdasarkan `bInterfaceProtocol` (1=keyboard,
/// 2=mouse). Cuma tertarik ke HID (class=3); device HID non-boot-protocol
/// (subclass != 1) tetap dilewatkan ke driver -- boot protocol di-set
/// eksplisit lewat SET_PROTOCOL di init_keyboard/init_mouse, jadi cukup
/// filter class di sini. Return jenis device yang berhasil di-dispatch
/// (dipakai caller buat nyatet has_keyboard/has_mouse di ActivePort),
/// atau `None` kalau endpoint ini bukan HID / gagal dikonfigurasi.
unsafe fn dispatch_endpoint(
    slot_id: u32,
    root_port: u8,
    ep_info: xhci::EndpointInfo,
) -> Option<DeviceKind> {
    unsafe {
        if ep_info.interface_class != 3 {
            return None;
        }

        let handle = match xhci::configure_endpoint(slot_id, root_port, ep_info) {
            Ok(h) => h,
            Err(e) => {
                println!("  configure_endpoint gagal: {}", e);
                return None;
            }
        };

        match ep_info.interface_protocol {
            1 => {
                keyboard_usb::init_keyboard(slot_id, handle, ep_info.inteface_number);
                Some(DeviceKind::Keyboard)
            }
            2 => {
                mouse_usb::init_mouse(slot_id, handle, ep_info.inteface_number);
                Some(DeviceKind::Mouse)
            }
            other => {
                println!(
                    "  HID interface protocol {:#04x} belum ada driver-nya, dilewati",
                    other
                );
                None
            }
        }
    }
}
