# Panduan Membuat Driver USB HID (Human Interface Device)

Panduan ini fokus ke lapisan **class driver HID** — asumsinya transport xHCI
(slot enable, address device, configure endpoint) sudah beres seperti di
`xhci.rs`. Ini generalisasi dari `keyboard_usb.rs` supaya bisa dipakai juga
untuk mouse, gamepad, atau HID generik lain.

---

## 0. Prasyarat dari Layer xHCI

Sebelum mulai kode HID, driver class butuh 3 hal ini sudah tersedia dari
transport layer:

```rust
xhci::enable_slot()                 -> slot_id
xhci::address_device(slot_id, ...)  -> device siap di-address
xhci::get_configuration_descriptor_and_find_interrupt_in(slot_id) -> EndpointInfo
xhci::configure_endpoint(slot_id, ep_info) -> KeyboardEndpoint { ring_virt, ring_phys, dci }
```

Kalau salah satu ini belum sukses, jangan lanjut ke kode HID — endpoint
belum ada, ring belum di-DMA-map ke controller.

---

## 1. Konsep Inti HID: Boot Protocol vs Report Protocol

Dua level "kemudahan" saat baca HID device:

| Mode | Format data | Kapan dipakai |
|---|---|---|
| **Boot Protocol** | Format tetap & terstandar (keyboard 8 byte, mouse 3-4 byte) | Paling gampang untuk driver hobby OS — tidak perlu parse HID Report Descriptor |
| **Report Protocol** | Format bebas, dideskripsikan lewat HID Report Descriptor | Perlu HID Report Descriptor parser (lebih rumit, tapi wajib untuk device non-boot seperti gamepad) |

**Rekomendasi**: mulai dari Boot Protocol dulu (seperti driver keyboard yang
sudah ada), baru upgrade ke Report Protocol kalau butuh device yang lebih
kompleks.

Set Boot Protocol via **class-specific control request**:

```rust
const HID_SET_PROTOCOL: u8 = 0x0B;
const HID_BOOT_PROTOCOL: u16 = 0;

xhci::control_transfer_no_data(
    slot_id,
    0x21,               // bmRequestType: Host->Device, Class, Interface
    HID_SET_PROTOCOL,
    HID_BOOT_PROTOCOL,
    interface_number as u16,
);
```

Request class-specific HID lain yang berguna:

| bRequest | Nilai | Fungsi |
|---|---|---|
| `GET_REPORT` | 0x01 | Baca laporan sekarang (polling manual, tanpa nunggu interrupt) |
| `SET_REPORT` | 0x09 | Kirim data ke device (misal set LED Num Lock/Caps Lock di keyboard) |
| `GET_IDLE` / `SET_IDLE` | 0x02/0x0A | Atur seberapa sering device kirim laporan walau tidak ada perubahan |
| `SET_PROTOCOL` | 0x0B | Pilih Boot vs Report protocol |

---

## 2. Struktur Report (Boot Protocol)

### 2.1 Keyboard (8 byte, standar)
```rust
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KeyboardReport {
    modifier: u8,      // bitmask: Ctrl/Shift/Alt/GUI kiri-kanan
    reserved: u8,
    keycodes: [u8; 6], // hingga 6 tombol ditekan bersamaan (n-key rollover terbatas)
}
```

### 2.2 Mouse (3-4 byte, standar boot mouse)
```rust
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MouseReport {
    buttons: u8,   // bit0=left, bit1=right, bit2=middle
    x: i8,         // delta gerakan X (relative)
    y: i8,         // delta gerakan Y (relative)
    wheel: i8,     // opsional, tidak semua device kirim ini
}
```
`REPORT_LEN` untuk mouse biasanya 3 atau 4 — cek dari `max_packet_size` dan
`actual_len` yang balik dari transfer, jangan asumsikan hardcode kalau device
tidak dikenal.

### 2.3 HID generik lain
Kalau bukan boot device, ukuran & layout report **wajib** didapat dari
parsing HID Report Descriptor (`GET_DESCRIPTOR` dengan `wValue = (0x22 <<
8)`, tipe `HID_REPORT`). Ini di luar cakupan boot protocol dan butuh state
machine parser usage/tag terpisah — skip dulu kalau baru mulai.

---

## 3. Pola Umum State Driver

Ikuti pola `KeyboardState` di `keyboard_usb.rs` — satu struct berisi semua
state, dibungkus `Mutex<Option<T>>` global supaya aman diakses dari main
loop **dan** dari interrupt handler tanpa `static mut`:

```rust
struct HidDeviceState {
    slot_id: u32,
    ring_virt: u64,
    ring_phys: u64,
    enqueue_index: usize,
    cycle_state: bool,
    dci: u32,
    report_buf_virt: u64,
    report_buf_phys: u64,
    last_report: ReportType, // buat deteksi perubahan/debounce
}

static HID_STATE: Mutex<Option<HidDeviceState>> = Mutex::new(None);
```

Kenapa harus `Mutex`, bukan variabel biasa: interrupt handler (di vector 44)
memanggil `poll_keyboard()`-mu secara asynchronous relatif terhadap main
loop — race condition kalau pakai `static mut`.

---

## 4. Alur Init Driver (`init_hid_device`)

Pola persis dari `init_keyboard`, digeneralisasi:

```rust
pub unsafe fn init_hid_device(slot_id: u32, ep: KeyboardEndpoint, interface_number: u8) {
    unsafe {
        // 1. set boot protocol (opsional, tergantung device)
        let _ = xhci::control_transfer_no_data(
            slot_id, 0x21, HID_SET_PROTOCOL, HID_BOOT_PROTOCOL, interface_number as u16,
        );

        // 2. alokasikan report buffer SEKALI, dipakai berulang
        let buf: Box<[u8; REPORT_LEN]> = Box::new([0u8; REPORT_LEN]);
        let virt = Box::into_raw(buf) as u64;
        let phys = memory::virtual_to_physical(virt as usize).expect("gagal translate buffer");

        // 3. simpan DCI endpoint ini ke tempat yang bisa dibaca interrupt handler
        xhci::KBD_DCI.store(ep.dci, Ordering::SeqCst); // atau variabel khusus device ini

        // 4. simpan state
        *HID_STATE.lock() = Some(HidDeviceState { slot_id, ring_virt: ep.ring_virt, .. });

        // 5. submit transfer pertama supaya device mulai kirim data
        arm_next_report();
    }
}
```

### 4.1 Kenapa harus "arm" (submit) transfer duluan?
Endpoint interrupt IN **tidak otomatis** mengirim data ke host. Host harus
submit Normal TRB ke ring endpoint tersebut lebih dulu (istilahnya
"priming"/"arming") — baru device akan mengisi buffer itu saat event
berikutnya terjadi (misal ada tombol ditekan, mouse digerakkan).

```rust
unsafe fn arm_next_report() {
    let mut state_guard = HID_STATE.lock();
    let state = state_guard.as_mut().unwrap();
    let xhci_guard = XHCI_INSTANCE.get().unwrap().lock();
    xhci_guard.enqueue_normal_trb_and_ring(
        state.ring_virt, state.ring_phys,
        &mut state.enqueue_index, &mut state.cycle_state,
        RING_SIZE, state.slot_id, state.dci,
        state.report_buf_phys, REPORT_LEN as u32,
    );
}
```

---

## 5. Alur Handling di Interrupt (`poll_hid_device`)

Dipanggil dari `common_interrupt_handler` saat vector xHCI (44) masuk —
lihat pola di `interrupt.rs`:

```rust
44 => {
    xhci.poll_event_ring();        // proses Transfer Completion Event
    keyboard_usb::poll_keyboard(); // baca data & re-arm
}
```

```rust
pub unsafe fn poll_hid_device() {
    unsafe {
        if !xhci::KBD_REPORT_PENDING.swap(false, Ordering::SeqCst) {
            return; // belum ada data baru
        }

        let buf_virt = { HID_STATE.lock().as_ref().unwrap().report_buf_virt };
        let report = (buf_virt as *const ReportType).read_volatile();

        handle_report(&report);

        arm_next_report(); // WAJIB, atau device berhenti kirim data
    }
}
```

**Poin paling sering bikin bug**: kalau lupa `arm_next_report()` di akhir,
device cuma kirim laporan sekali lalu diam total — kelihatannya seperti
device "mati" padahal drivernya yang lupa priming ulang.

---

## 6. Pola Deteksi Perubahan (Debounce / Edge Detection)

### Keyboard: deteksi "key baru ditekan" vs "masih ditahan"
```rust
for &kc in &report.keycodes {
    if kc == 0 { continue; }
    if state.last_keycodes.contains(&kc) {
        continue; // masih ditahan dari report sebelumnya, jangan trigger ulang
    }
    // proses key baru
}
state.last_keycodes = report.keycodes;
```

### Mouse: delta langsung dipakai, tidak butuh "last state" untuk gerakan
```rust
if report.x != 0 || report.y != 0 {
    cursor_x += report.x as i32;
    cursor_y += report.y as i32;
}
// tapi tombol tetap butuh edge detection seperti keyboard
let newly_pressed = report.buttons & !state.last_buttons;
let newly_released = !report.buttons & state.last_buttons;
state.last_buttons = report.buttons;
```

---

## 7. Menerjemahkan Keycode → Karakter (khusus keyboard)

HID keycode adalah nilai standar USB HID Usage Table, bukan ASCII langsung
— wajib tabel translasi:

```rust
fn hid_keycode_to_ascii(keycode: u8, modifier: u8) -> Option<char> {
    let shift = modifier & 0x22 != 0; // bit1=Left Shift, bit5=Right Shift
    match keycode {
        0x04..=0x1D => { /* a-z */ }
        0x1E..=0x27 => { /* 1-9,0 + simbol shift */ }
        0x2C => Some(' '),
        0x28 => Some('\n'),
        0x2A => Some('\u{8}'), // backspace
        0x2B => Some('\t'),
        _ => None,
    }
}
```

Untuk layout non-US atau karakter khusus, tabel ini harus diperluas —
referensi lengkap ada di *USB HID Usage Tables* spec, section "Keyboard/
Keypad Page".

---

## 8. Checklist Khusus HID

- [ ] `SET_PROTOCOL` ke Boot Protocol dikirim sebelum mulai baca report
- [ ] Report buffer dialokasikan **sekali**, bukan tiap poll
- [ ] Transfer pertama di-*arm* sebelum menunggu interrupt
- [ ] Setiap selesai proses report, **re-arm** transfer berikutnya
- [ ] State driver pakai `Mutex`, bukan `static mut`, karena diakses dari
      interrupt context
- [ ] Edge detection (bandingkan dengan laporan sebelumnya) untuk tombol,
      supaya tidak re-trigger selama tombol ditahan
- [ ] Kalau device bukan boot-compatible (banyak gamepad/device custom),
      jangan asumsikan layout report tetap — perlu parsing HID Report
      Descriptor dulu

---

## 9. Kerangka Kode Minimal (ringkasan siap-pakai)

```rust
pub unsafe fn init_hid_device(slot_id: u32, ep: KeyboardEndpoint, iface: u8) {
    unsafe {
        let _ = xhci::control_transfer_no_data(slot_id, 0x21, 0x0B, 0, iface as u16);
        let buf: Box<[u8; REPORT_LEN]> = Box::new([0u8; REPORT_LEN]);
        let virt = Box::into_raw(buf) as u64;
        let phys = memory::virtual_to_physical(virt as usize).unwrap();
        *HID_STATE.lock() = Some(HidDeviceState {
            slot_id, ring_virt: ep.ring_virt, ring_phys: ep.ring_phys,
            enqueue_index: 0, cycle_state: true, dci: ep.dci,
            report_buf_virt: virt, report_buf_phys: phys as u64,
            last_report: Default::default(),
        });
        arm_next_report();
    }
}

pub unsafe fn poll_hid_device() {
    unsafe {
        if !REPORT_PENDING.swap(false, Ordering::SeqCst) { return; }
        let buf_virt = HID_STATE.lock().as_ref().unwrap().report_buf_virt;
        let report = (buf_virt as *const ReportType).read_volatile();
        handle_report(&report);
        arm_next_report();
    }
}
```

Ganti `ReportType` dan `handle_report` sesuai device (keyboard, mouse, atau
HID lain) — struktur transport (arm → interrupt → baca → re-arm) tetap sama
untuk semua HID boot device.
