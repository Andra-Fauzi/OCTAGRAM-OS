# Dokumentasi Driver xHCI & USB Keyboard — OCTAGRAM-OS

Dokumen ini menjelaskan dari nol bagaimana driver xHCI dan keyboard USB di kernel kamu bekerja: konsep dasarnya, struktur datanya, alur kodenya langkah demi langkah, dan kenapa desainnya seperti itu. Ditulis dengan asumsi kamu belum familiar sama sekali dengan xHCI.

---

## Daftar Isi

1. [Apa itu xHCI dan kenapa dibutuhkan](#1-apa-itu-xhci-dan-kenapa-dibutuhkan)
2. [Istilah-istilah dasar](#2-istilah-istilah-dasar)
3. [Peta memori xHCI](#3-peta-memori-xhci)
4. [Struktur data inti](#4-struktur-data-inti)
5. [Ring buffer, cycle bit, dan Link TRB](#5-ring-buffer-cycle-bit-dan-link-trb)
6. [Alur inisialisasi controller](#6-alur-inisialisasi-controller)
7. [Alur enumerasi device (dari colokan sampai siap pakai)](#7-alur-enumerasi-device-dari-colokan-sampai-siap-pakai)
8. [Interrupt, MSI-X, dan APIC](#8-interrupt-msi-x-dan-apic)
9. [Driver keyboard HID](#9-driver-keyboard-hid)
10. [Model concurrency: Mutex, cli/sti](#10-model-concurrency-mutex-clisti)
11. [Riwayat bug yang pernah terjadi](#11-riwayat-bug-yang-pernah-terjadi)
12. [Diagram alur keseluruhan](#12-diagram-alur-keseluruhan)
13. [Ide pengembangan lanjutan](#13-ide-pengembangan-lanjutan)
14. [Peta file ke fungsi](#14-peta-file-ke-fungsi)

---

## 1. Apa itu xHCI dan kenapa dibutuhkan

**xHCI** (eXtensible Host Controller Interface) adalah spesifikasi hardware buat USB 3.x (dan tetap kompatibel USB 2.0/1.1) yang menggantikan standar lama seperti UHCI/OHCI/EHCI. Ini adalah **chip di motherboard** yang jadi jembatan antara CPU dan semua port USB fisik.

Kenapa OS butuh "driver" buat ini? Karena tanpa xHCI di-drive dengan benar:
- Port USB gak akan mendeteksi device yang dicolok.
- Data gak bisa dikirim/diterima ke/dari device USB (keyboard, mouse, flashdisk, dst).

Beda dengan PS/2 keyboard (yang komunikasinya sederhana lewat 1-2 I/O port), **USB itu protokol berlapis dan kompleks**: ada lapisan fisik, lapisan transaksi, lapisan device (deskriptor, konfigurasi), sampai lapisan class-specific (HID, Mass Storage, dst). xHCI menyembunyikan sebagian besar kerumitan sinyal fisik, tapi OS tetap harus bicara ke controller lewat **struktur data di memori** (bukan langsung I/O port seperti PS/2) — ini yang bikin drivernya terasa berat di awal.

---

## 2. Istilah-istilah dasar

| Istilah | Penjelasan singkat |
|---|---|
| **Host Controller (xHC)** | Chip xHCI itu sendiri. Kamu bicara dengannya lewat register MMIO (alamat memori khusus yang dipetakan ke hardware). |
| **BAR (Base Address Register)** | Alamat fisik dasar dari region memori xHC, didapat dari config space PCI. |
| **Slot** | "ID" yang xHC berikan untuk tiap device USB yang berhasil dikenali. Slot ini bukan device fisik, tapi representasi logis di sisi controller. |
| **Endpoint** | Saluran komunikasi ke/dari device. Satu device bisa punya banyak endpoint (misal EP0 buat kontrol, EP lain buat data). Tiap endpoint punya arah (IN = device→host, OUT = host→device) dan tipe (Control, Interrupt, Bulk, Isochronous). |
| **DCI (Device Context Index)** | Nomor unik tiap endpoint dalam satu device (dihitung dari nomor endpoint + arah). |
| **TRB (Transfer Request Block)** | Unit data 16 byte yang jadi "instruksi" — baik dari software ke hardware (command/transfer) maupun dari hardware ke software (event). Semua komunikasi xHCI dibungkus TRB. |
| **Ring** | Array melingkar berisi banyak TRB berurutan di memori, dibaca berurutan oleh xHC. Ada 3 jenis: Command Ring, Transfer Ring (per endpoint), Event Ring. |
| **Doorbell** | Register MMIO yang kamu "ketuk" (tulis nilai) untuk memberi tahu xHC "ada TRB baru, tolong diproses". |
| **DCBAA (Device Context Base Address Array)** | Array pointer, satu per slot, menunjuk ke Device Context masing-masing device. |
| **Device Context** | Struct berisi state device dari sudut pandang xHC: status slot dan status tiap endpoint-nya. |
| **Input Context** | Struct yang kamu isi buat *meminta* xHC mengubah Device Context (dipakai saat Address Device / Configure Endpoint). |
| **Cycle bit** | 1 bit penanda "TRB ini valid/baru" di tiap TRB — dipakai supaya software dan hardware tahu sampai mana ring sudah diproses tanpa perlu pointer terpisah yang di-share. |
| **MSI-X** | Mekanisme interrupt modern (pengganti IRQ line lama) — xHC mengirim interrupt lewat "menulis pesan" ke memori, bukan menaikkan pin fisik. |
| **HID** | USB Human Interface Device class — standar untuk keyboard, mouse, gamepad, dll. Keyboard di kode ini pakai "Boot Protocol" HID, versi paling sederhana dan seragam. |

---

## 3. Peta memori xHCI

Semua interaksi dengan xHC lewat 4 kelompok register, semuanya diakses lewat MMIO (baca/tulis alamat memori, bukan `in`/`out` instruction seperti PS/2):

```
BAR0 (physical) ──map_mmio_page──▶ virt_base
                                      │
      ┌───────────────────────────────┼────────────────────────────────┐
      │                                │                                │
      ▼                                ▼                                ▼
Capability Registers          Operational Registers            Runtime Registers
(read-only, offset 0)         (offset = CAPLENGTH)             (offset = RTSOFF)
- CAPLENGTH                   - USBCMD (start/stop/reset)      - Interrupter 0..N
- HCIVERSION                  - USBSTS (status)                  - IMAN, IMOD
- HCSPARAMS1 (max slot/port)  - CONFIG (max slot aktif)          - ERSTSZ, ERSTBA
- HCCPARAMS1 (fitur 64-bit)   - DCBAAP (pointer ke DCBAA)         - ERDP
- DBOFF (offset doorbell)     - CRCR (pointer command ring)
- RTSOFF (offset runtime)     - PORTSC[n] (status tiap port)
                                                                Doorbell Registers
                                                                (offset = DBOFF)
                                                                - Doorbell[0] = command ring
                                                                - Doorbell[1..N] = per slot/endpoint
```

Di kode kamu, ini dipetakan lewat struct `XhciCapRegs`, `XhciOpRegs`, `XhciInterrupterRegs` di `xhci.rs` — masing-masing cuma "view" (pointer + helper baca/tulis offset tertentu) ke area memori yang sama, gak menyimpan data sendiri.

---

## 4. Struktur data inti

Semua "percakapan" dengan xHC lewat lima struktur data di memori berikut. Semuanya dialokasikan dengan `Box`, lalu alamat virtualnya diterjemahkan ke alamat fisik (`memory::virtual_to_physical`) karena **xHC hanya paham alamat fisik**, bukan alamat virtual CPU.

### a. DCBAA (Device Context Base Address Array)
Array `[u64; 256]` — index-nya adalah slot ID, isinya pointer fisik ke Device Context device itu. xHC baca array ini tiap kali butuh tahu state device di slot tertentu.

### b. Command Ring
Ring TRB tempat kamu kirim **perintah global** ke controller: `Enable Slot`, `Address Device`, `Configure Endpoint`, dll. Selalu diproses lewat doorbell 0.

### c. Event Ring (+ ERST)
Ring tempat **xHC menulis hasil/notifikasi** ke software — kebalikan arah dari Command/Transfer Ring. ERST (Event Ring Segment Table) adalah "daftar isi" yang bilang ke xHC di mana segmen event ring berada dan seberapa besar.

### d. Device Context & Input Context
- **Device Context** = "rapor" device menurut xHC (read oleh software, ditulis oleh xHC).
- **Input Context** = "formulir permintaan perubahan" yang kamu isi lalu kirim lewat command `Address Device` / `Configure Endpoint`. Isinya mirror dari Device Context tapi ditambah `InputControlContext` (bilang field mana saja yang mau diubah).

### e. Transfer Ring (satu per endpoint)
Ring tempat kamu kirim TRB data untuk endpoint tertentu (bukan command global). EP0 (control endpoint) selalu ada satu, dan tiap endpoint tambahan yang di-configure (misalnya endpoint interrupt keyboard) punya ring sendiri.

```
                     ┌────────────────────┐
                     │      DCBAA          │  (1 array, index = slot id)
                     └─────────┬──────────┘
                               │ pointer per slot
                               ▼
                     ┌────────────────────┐
                     │  Device Context     │  (per device)
                     │  - Slot Context     │
                     │  - EP Context x31   │───▶ tr_dequeue_ptr ──▶ Transfer Ring EPx
                     └────────────────────┘
```

---

## 5. Ring buffer, cycle bit, dan Link TRB

Ini bagian paling sering jadi sumber bug (dan memang dua bug besar yang kita perbaiki bersumber dari sini), jadi dijelaskan detail.

### Kenapa butuh "cycle bit"?

Ring itu memori melingkar yang dipakai bersama oleh software (producer, nulis TRB baru) dan hardware (consumer, baca TRB). Supaya keduanya sinkron **tanpa perlu terus-menerus baca-tulis pointer bersama** (yang mahal & rawan race), dipakai trik: tiap TRB punya 1 bit "cycle". Nilai cycle yang *diharapkan* berbalik (toggle) tiap kali ring penuh satu putaran. Consumer cuma perlu bandingkan cycle bit TRB yang dia baca dengan cycle bit yang dia "harapkan" saat ini — kalau beda, artinya "belum ada TRB baru di situ, berhenti dulu".

Ini persis logika di `poll_event_ring`:
```rust
let trb_cycle_bit = (trb.control & 0x1) != 0;
if trb_cycle_bit != self.cycle_state {
    return; // belum ada event baru
}
```

### Kenapa butuh Link TRB?

Ring di memori itu **array linear biasa**, bukan struktur melingkar beneran. Waktu producer (software) sudah mengisi TRB terakhir dan mau "muter balik" ke index 0, **hardware yang membaca TRB satu-satu secara linear gak otomatis tahu itu**. Kalau tidak diberi tahu secara eksplisit, hardware akan lanjut baca alamat memori setelah TRB terakhir — yang isinya bukan bagian dari ring, alias sampah/undefined.

Solusinya: taruh 1 TRB khusus bertipe **Link TRB** di slot terakhir ring, isinya "alamat fisik untuk lompat balik ke awal ring". Waktu hardware sampai di situ, dia tahu harus loncat ke awal ring, bukan lanjut baca memori berikutnya.

```
Tanpa Link TRB (BUG):
[data][data][data]...[data][ ?? sampah ?? ]  ◀── hardware macet di sini
 idx0                      idx15

Dengan Link TRB (BENAR):
[data][data][data]...[data][ LINK → balik ke idx0 ]
 idx0                      idx15
```

Ring yang **wajib** punya Link TRB kalau dipakai berkali-kali sampai wrap: Command Ring, tiap Transfer Ring (EP0 dan endpoint lain seperti keyboard). Event Ring **tidak perlu** Link TRB manual — wraparound-nya sudah ditangani otomatis oleh xHC lewat mekanisme ERST.

---

## 6. Alur inisialisasi controller

Fungsi utamanya `init_xhci()` di `xhci.rs`, dipanggil dari `main.rs`. Urutannya:

1. **Reset controller** — clear bit Run/Stop di `USBCMD`, tunggu `USBSTS.HCH` (Halted) jadi 1, lalu set bit `HCRST` (Host Controller Reset), tunggu sampai reset selesai dan `USBSTS.CNR` (Controller Not Ready) jadi 0.
2. **Baca MaxSlots** dari `HCSPARAMS1`, tulis ke register `CONFIG` — ini bilang ke controller berapa banyak device yang mau kamu dukung sekaligus.
3. **Alokasi & daftarkan DCBAA** — alokasi array 256 pointer, terjemahkan ke alamat fisik, tulis ke register `DCBAAP`.
4. **Alokasi & daftarkan Command Ring** — sama pola: alokasi, translate fisik, tulis ke register `CRCR` (bit paling bawah = cycle state awal, selalu 1).
5. **Setup Event Ring** (`setup_event_ring`) — alokasi segmen event ring + ERST (1 entry), tulis `ERSTSZ`, `ERDP`, `ERSTBA` ke Interrupter 0, dan set bit interrupt enable (`IMAN`).
6. **Start controller** (`start_controller`) — set bit `INTE` (Interrupt Enable) dan `Run/Stop` di `USBCMD`, tunggu `USBSTS.HCH` jadi 0 (artinya controller sudah benar-benar jalan).

Setelah tahap ini, controller sudah "hidup" tapi belum ada device yang dikenali — itu baru terjadi di tahap enumerasi.

---

## 7. Alur enumerasi device (dari colokan sampai siap pakai)

Ini yang terjadi tiap kali ada device USB baru yang mau dipakai (misalnya keyboard). Urutan lengkapnya, dengan fungsi yang bersangkutan:

| # | Langkah | Fungsi | Penjelasan |
|---|---|---|---|
| 1 | Scan port | `scan_ports` | Baca `PORTSC` tiap port, cek bit "connected". Log kamu menunjukkan `Port 4 CONNECTED, speed=3`. |
| 2 | Minta slot baru | `enable_slot` | Kirim command `Enable Slot` lewat Command Ring, xHC balas dengan Slot ID (di log: `slot_id=1`). |
| 3 | Alamatkan device | `address_device` | Isi Input Context (info port, speed, EP0), kirim command `Address Device`. xHC sekarang menganggap device "punya alamat" dan EP0 siap dipakai. |
| 4 | Ambil Device Descriptor | `get_device_descriptor` | Control transfer standar `GET_DESCRIPTOR` lewat EP0 — hasilnya vendor ID, product ID, class, max packet size EP0, dst. |
| 5 | Ambil Configuration Descriptor | `get_configuration_descriptor_and_find_interrupt_in` | Ambil deskriptor konfigurasi (berisi info semua interface & endpoint), parse manual byte-per-byte, cari endpoint interrupt IN (yang dipakai keyboard kirim laporan tombol). |
| 6 | Set Configuration | `control_transfer_no_data(..., 0x09, 1, 0)` | Request standar `SET_CONFIGURATION` — device resmi "aktif" pakai konfigurasi nomor 1. |
| 7 | Configure Endpoint | `configure_endpoint` | Isi Input Context Full (slot + endpoint context untuk endpoint interrupt yang ditemukan), kirim command `Configure Endpoint`. Ini yang bikin xHC benar-benar menyiapkan Transfer Ring untuk endpoint tersebut. |
| 8 | Set Boot Protocol (khusus HID) | `init_keyboard` → `control_transfer_no_data` dengan `bRequest=0x0B` | Request class-specific HID `SET_PROTOCOL` supaya device kirim laporan dalam format "Boot Protocol" yang seragam (8 byte tetap), bukan format custom vendor. |
| 9 | Mulai polling | `init_keyboard` → `arm_next_report` | Kirim TRB pertama ke Transfer Ring endpoint interrupt, nunggu device kirim laporan tombol. |

### Kenapa semua step ini harus lewat command/control transfer, bukan langsung tulis field?

Karena **Device Context itu "milik" xHC**, software tidak boleh menulis langsung ke situ (selain lewat command resmi). Semua perubahan harus lewat Input Context + command, supaya xHC bisa validasi dan menjaga state internalnya tetap konsisten.

---

## 8. Interrupt, MSI-X, dan APIC

### Kenapa tidak polling terus-menerus?

Bisa saja terus-menerus cek (`poll_event_ring` di loop tanpa henti) — dan itu memang fallback kalau MSI-X gagal di-setup (lihat cabang `else` di `main.rs`). Tapi ini boros CPU dan gak scalable. Solusi standarnya: **interrupt** — CPU "tidur" (`hlt`) sampai ada kejadian, baru dibangunkan.

### Alur MSI-X di kode ini

1. `setup_msix` menulis alamat tujuan (`0xFEE0_0000 | apic_id<<12`) dan vector number (44) ke MSI-X Table milik device xHC lewat BAR-nya sendiri, lalu enable MSI-X di PCI config space.
2. Saat xHC menaruh event baru di Event Ring, dia langsung "menulis" pesan interrupt ke Local APIC CPU — ini yang memicu CPU meloncat ke IDT vector 44.
3. `interrupt.rs` punya stub assembly (`xhci_irq_stub`, hasil macro `interrupt_stub!`) yang menyimpan semua register, lalu panggil `common_interrupt_handler`.
4. Untuk vector 44 spesifik: `poll_event_ring()` dipanggil (proses 1 event dari Event Ring), lalu `keyboard_usb::poll_keyboard()` dipanggil (cek apakah event tadi laporan keyboard, kalau ya proses & re-arm).
5. Di akhir handler, `lapic.send_eoi()` dipanggil — **wajib**, ini memberi tahu Local APIC "interrupt sudah selesai ditangani, boleh kirim interrupt berikutnya". Kalau ini lupa, interrupt berikutnya gak akan pernah masuk.

### Kenapa IDT vector harus Interrupt Gate, bukan Trap Gate?

Lihat di `idt.rs`: `type_attr: 0x8E`. Bit ini bikin CPU otomatis **clear flag IF (Interrupt Enable)** begitu masuk handler — jadi selama handler jalan, tidak ada interrupt lain yang bisa menyela (mencegah re-entrancy yang bisa merusak state Mutex/ring). IF dipulihkan otomatis oleh `iretq` di akhir stub.

---

## 9. Driver keyboard HID

File: `keyboard_usb.rs`.

### Format laporan (report) HID Boot Keyboard

Setiap laporan yang dikirim keyboard selalu **8 byte tetap**:

```
byte 0     : modifier keys (bit-field)
             bit0=LeftCtrl bit1=LeftShift bit2=LeftAlt bit3=LeftGUI
             bit4=RightCtrl bit5=RightShift bit6=RightAlt bit7=RightGUI
byte 1     : reserved (selalu 0)
byte 2-7   : sampai 6 keycode yang SEDANG ditekan bersamaan (6KRO)
```

Kode kamu (`KeyboardReport` struct) memetakan langsung 8 byte ini.

### Kenapa harus terus "re-arm"?

Tiap TRB yang kamu kirim ke Transfer Ring endpoint interrupt itu **sekali pakai** — begitu device mengisi 1 laporan ke buffer dan xHC mengirim Transfer Completion Event, TRB itu sudah "habis". Supaya device bisa kirim laporan berikutnya (misalnya tombol berikutnya ditekan), kamu **wajib** kirim TRB baru lagi (`arm_next_report`). Kalau lupa, keyboard berhenti total mengirim laporan setelah 1 kali — inilah yang harus selalu dipanggil setelah tiap laporan diproses.

### Kenapa ada 2 laporan per 1 kali pencet tombol?

Device USB HID mengirim laporan setiap kali **state tombol berubah**, bukan cuma sekali per pencet. Jadi 1 kali tekan-lepas tombol = minimal 2 laporan: satu waktu ditekan (keycode terisi), satu waktu dilepas (semua keycode balik 0). Kode kamu (`handle_report`) sengaja skip keycode 0 supaya tidak mencetak karakter dobel, tapi laporan "lepas" itu tetap harus diterima & di-re-arm seperti biasa.

### Kenapa perlu cek `last_keycodes` (state sebelumnya)?

Karena keyboard biasanya mengirim laporan berulang selama tombol ditahan (auto-repeat di level USB), bukan cuma sekali. Kalau tidak dicek, satu kali tekan-tahan bisa mencetak karakter berkali-kali dalam waktu sangat singkat. `handle_report` membandingkan dengan laporan sebelumnya supaya cuma trigger cetak karakter sekali per "tombol baru ditekan".

### Kenapa `KBD_DCI` dipakai untuk membedakan event?

Event Ring itu **satu untuk semua endpoint** (EP0, endpoint interrupt keyboard, semuanya lewat ring yang sama). Supaya `poll_event_ring` tahu event yang baru masuk itu punya siapa, dia baca field **Endpoint ID** di TRB event (`(trb.control >> 16) & 0x1F`) dan bandingkan dengan DCI keyboard yang disimpan di `KBD_DCI`. Kalau cocok → itu laporan keyboard, diarahkan ke jalur `KBD_REPORT_PENDING`. Kalau tidak → itu completion dari control transfer biasa (EP0), diarahkan ke jalur `TRANSFER_COMPLETION_PENDING`.

---

## 10. Model concurrency: Mutex, cli/sti

Kernel ini **single-core** dan pakai kombinasi dua mekanisme proteksi:

1. **`asm!("cli")` / `asm!("sti")`** — mematikan/menyalakan interrupt secara eksplisit di sekitar kode yang mengirim command/transfer dari *main context* (bukan dari dalam interrupt handler). Ini mencegah interrupt (termasuk vector 44) menyela di tengah-tengah kode yang sedang memegang lock, yang kalau dibiarkan bisa bikin deadlock (karena `spin::Mutex` bukan "interrupt-safe" — dia cuma spin/busy-loop menunggu, tidak tahu soal interrupt).
2. **`spin::Mutex`** (dipakai buat `XHCI_INSTANCE` dan `KBD_STATE`) — melindungi data supaya tidak diakses dari dua "pihak" sekaligus (main context dan interrupt handler) dengan cara yang saling tabrakan.

Pola yang konsisten dipakai di semua fungsi command/transfer:
```rust
core::arch::asm!("cli");
{
    let mut guard = XHCI_INSTANCE.get().unwrap().lock();
    // ...kirim TRB, ring doorbell...
} // <-- lock dilepas di sini SEBELUM sti, supaya interrupt yang mungkin
  //     langsung nyala gak nabrak lock yang masih dipegang
core::arch::asm!("sti");
```

**Penting:** karena IDT vector 44 pakai Interrupt Gate (lihat bagian 8), selama handler interrupt itu jalan, IF otomatis 0 — jadi *dalam* handler sendiri interrupt lain tidak bisa menyela, dan kode di dalamnya (`poll_keyboard`, `arm_next_report`, dst) **tidak perlu** `cli`/`sti` manual lagi.

---

## 11. Riwayat bug yang pernah terjadi

Dua bug besar yang sudah ditemukan dan diperbaiki dalam pengembangan ini, dicatat supaya jadi referensi kalau muncul gejala serupa lagi:

### Bug #1 — Status Stage TRB hilang di `control_transfer_no_data`
**Gejala:** panic `Timeout control transfer no data` saat memanggil `SET_CONFIGURATION`.
**Sebab:** control transfer tanpa data tetap wajib punya 2 tahap: Setup Stage lalu Status Stage. Kode awal cuma mengirim Setup Stage TRB, lalu langsung ring doorbell — xHC tidak pernah menyelesaikan transaksi karena tahap Status-nya tidak pernah dikirim, jadi tidak pernah ada Transfer Completion Event.
**Perbaikan:** tambahkan Status Stage TRB (`TRB_TYPE_STATUS_STAGE`, arah IN karena tidak ada data stage) sebelum ring doorbell.

### Bug #2 — Ring interrupt endpoint tidak punya Link TRB fisik
**Gejala:** keyboard berhenti merespons total setelah sekitar 8 karakter diketik.
**Sebab:** ring Transfer untuk endpoint interrupt (dan ring lain: EP0, command ring) di-desain untuk wrap balik ke index 0 setelah slot terakhir terpakai, tapi **slot terakhir tidak pernah diisi TRB Link sungguhan**. Karena tiap tombol (tekan + lepas) memakai 1 TRB, sekitar 15-16 TRB terpakai dalam beberapa detik ketikan — begitu wrap terjadi, xHC membaca slot kosong/sampah dan berhenti total memproses ring itu.
**Perbaikan:** tulis Link TRB asli (`TRB_TYPE_LINK`, dengan bit Toggle Cycle) di slot terakhir tiap ring saat dialokasikan, dan refresh cycle bit-nya tiap kali producer software melewati titik wrap.

> Catatan: Command Ring dan Transfer Ring EP0 punya potensi bug yang sama (belum ada Link TRB), tapi belum kejadian karena jumlah command/control-transfer yang dikirim masih sedikit dan belum sampai wrap. Kalau nanti fitur bertambah (banyak device, banyak request), pola perbaikan yang sama perlu diterapkan di situ juga.

---

## 12. Diagram alur keseluruhan

```
 main.rs (kmain)
    │
    ├─ gdt::load_gdt() ─────────────────▶ setup segmentasi CPU
    ├─ idt::load_idt() ─────────────────▶ daftarkan semua interrupt handler
    ├─ apic::init_lapic()/init_ioapic() ▶ siapkan penerima interrupt
    ├─ pci::scan_pci_bus()
    ├─ pci::find_usb_controllers() ─────▶ ketemu xHC di 00:03.0
    │
    ├─ XHCI::new()               (map BAR0 ke virtual memory)
    ├─ setup_msix()               (daftarkan vector 44 ke device)
    ├─ init_xhci()                (reset, DCBAA, command ring, event ring, RUN)
    ├─ scan_ports()                (device kedeteksi di port 4)
    │
    ├─ enable_slot()              ─┐
    ├─ address_device()             │  ENUMERASI
    ├─ get_device_descriptor()      │  (lihat bagian 7)
    ├─ get_configuration_descriptor_and_find_interrupt_in() │
    ├─ control_transfer_no_data(SET_CONFIGURATION)          │
    ├─ configure_endpoint()        ─┘
    │
    └─ keyboard_usb::init_keyboard()
          ├─ SET_PROTOCOL (Boot Protocol)
          └─ arm_next_report()   (kirim TRB pertama, tunggu device)

                         ⇣ (device kirim laporan tombol)

 [Hardware] xHC tulis Transfer Event TRB ke Event Ring
                         ⇣
 [Hardware] xHC kirim MSI-X ─────▶ Local APIC ─────▶ CPU loncat ke IDT[44]
                         ⇣
 interrupt.rs: xhci_irq_stub ─▶ stub_common ─▶ common_interrupt_handler
                         │
                         ├─ xhci.poll_event_ring()   (baca 1 event, tandai pending)
                         ├─ keyboard_usb::poll_keyboard()
                         │      ├─ baca buffer laporan
                         │      ├─ handle_report() ─▶ cetak karakter (kalau bukan report kosong/berulang)
                         │      └─ arm_next_report() (WAJIB, supaya bisa terima laporan berikutnya)
                         └─ lapic.send_eoi()          (WAJIB, supaya interrupt berikutnya bisa masuk)
```

---

## 13. Ide pengembangan lanjutan

Kalau mau dikembangkan lebih jauh, ini beberapa arah yang natural dari posisi sekarang:

- **Link TRB di semua ring** — terapkan juga ke Command Ring dan EP0 Ring supaya kernel tahan pakai jangka panjang tanpa macet mendadak (lihat catatan di bagian 11).
- **Dukungan multi-device** — saat ini `KeyboardState` cuma menyimpan 1 device. Untuk mendukung lebih dari satu keyboard/mouse, `KBD_STATE` perlu diganti jadi koleksi (misal `Vec<KeyboardState>` atau map per slot_id), dan `KBD_DCI` (single value) perlu diganti jadi lookup per slot.
- **Modifier state & lock keys** — belum ada penanganan Caps Lock/Num Lock (termasuk LED-nya, yang butuh `SET_REPORT` output report balik ke device).
- **Keymap layout** — saat ini keycode→karakter cuma untuk layout US sederhana; bisa dibuat konfigurable.
- **Reset Endpoint saat error** — kalau completion code bukan 1 (Success) atau 13 (Short Packet), endpoint bisa masuk state Halted dan butuh command `Reset Endpoint` sebelum dipakai lagi — belum ada penanganannya sekarang.
- **Mouse & device HID lain** — pola yang sama (enumerasi → configure endpoint interrupt → boot protocol → re-arm loop) bisa dipakai ulang untuk mouse (report 3-4 byte: tombol + delta X/Y).

---

## 14. Peta file ke fungsi

| File | Isi |
|---|---|
| `pci.rs` | Scan bus PCI, baca/tulis config space, cari xHC, baca BAR0 & ukurannya, setup MSI/MSI-X di level PCI. |
| `xhci.rs` | Inti driver: register view (`XhciCapRegs`/`XhciOpRegs`/`XhciInterrupterRegs`), struct `XHCI`, semua command & control transfer, ring buffer & Link TRB, `poll_event_ring`. |
| `keyboard_usb.rs` | Driver HID keyboard: state (`KeyboardState`), init & re-arm loop, parsing report, mapping keycode→karakter. |
| `interrupt.rs` | Stub assembly per vector interrupt, `common_interrupt_handler` yang me-routing ke driver yang sesuai berdasarkan nomor vector. |
| `apic.rs` | Setup Local APIC & I/O APIC, kirim EOI, disable PIC lama. |
| `idt.rs` / `gdt.rs` | Setup dasar CPU x86-64 (segmentasi & tabel interrupt) — prasyarat sebelum interrupt apa pun bisa jalan. |
| `main.rs` | Orkestrasi keseluruhan: urutan boot, panggil semua modul di atas sesuai urutan yang benar. |

---

*Dokumen ini dibuat berdasarkan kondisi kode per sesi debugging terakhir (setelah perbaikan bug Status Stage TRB dan Link TRB). Kalau kode berubah signifikan, bagian nomor baris/detail implementasi di atas perlu disesuaikan ulang.*
