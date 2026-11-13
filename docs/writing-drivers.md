# Membuat driver

Driver baru sebaiknya memisahkan akses hardware dari kebijakan kernel. Kode
arsitektur khusus CPU ada di `kernel/src/arch/<arch>/`; driver perangkat dan
kelas perangkat berada di `kernel/src/drivers/`; API reusable berada di
subsystem seperti `storage/`.

## Langkah umum

1. Identifikasi bus/perangkat dan sumber daya yang dimiliki: PCI BAR, port I/O,
   MMIO, IRQ/MSI, DMA, atau block device.
2. Buat modul baru pada lokasi yang sesuai dan daftarkan dengan `pub mod` pada
   `mod.rs` pemiliknya.
3. Implementasikan operasi hardware dalam tipe driver yang memiliki state
   eksplisit. Validasi register, ukuran buffer, alignment, timeout, dan status
   error; jangan mengasumsikan perangkat merespons.
4. Panggil inisialisasi dari orkestrasi boot atau jalur enumerasi bus yang
   sesuai. Hindari melakukan seluruh discovery di interrupt handler.
5. Untuk operasi asynchronous, handler IRQ sebaiknya acknowledge perangkat,
   menyimpan event ringkas, dan membangunkan task; kerjakan alokasi dan operasi
   kompleks di luar ISR.
6. Tambahkan tes host untuk parser/protokol/validasi input dengan fake device,
   lalu jalankan build bare-metal.

## Driver block

Untuk mengekspos media sektor ke FAT32, implementasikan trait
`storage::block::BlockDevice`:

```rust,ignore
impl BlockDevice for MyDisk {
    fn name(&self) -> &str { "disk0" }
    fn sector_count(&self) -> u64 { self.sectors }
    fn read_sectors(&self, lba: u64, buffer: &mut [u8])
        -> Result<(), BlockError> { /* validate and read */ }
    fn write_sectors(&self, lba: u64, buffer: &[u8])
        -> Result<(), BlockError> { /* validate and write */ }
}
```

Buffer length must be a multiple of 512, and `lba + count` must remain within
device capacity. Register an `Arc<dyn BlockDevice>` with
`storage::block::register`; the shell discovers registered devices through
`devices`. Follow the AHCI and USB mass-storage drivers for existing patterns.

## MMIO, DMA, dan locking

- Map MMIO through the paging helpers and use volatile accesses where required.
- Convert virtual and physical addresses deliberately; DMA addresses are not
  automatically equivalent to kernel virtual pointers.
- Keep DMA buffers aligned and alive while the controller can access them.
- Do not hold a spinlock while waiting indefinitely for hardware. Use bounded
  polling/timeout and return a typed error.
- Avoid printing, allocating, or taking long-held locks in an ISR.
- Document ownership and invariants around every unsafe register/DMA operation.

## Keyboard/IRQ pattern

The keyboard driver uses atomics for a bounded event queue in interrupt context.
The shell consumes queued events in a task. New interrupt handlers must match
the exact stack-frame convention in `arch/x86_64/interrupt.rs`, acknowledge the
interrupt controller, and avoid blocking. Add a host unit test for scan-code or
event translation rather than trying to emulate hardware in a unit test.

## Checklist sebelum merge

- [ ] Validasi semua input hardware dan batas indeks/alamat.
- [ ] Error perangkat diteruskan ke caller, tidak dianggap sukses.
- [ ] Tidak ada alokasi tak terbatas atau blocking di ISR.
- [ ] Resource dilepas pada jalur gagal dan unplug/hot-remove.
- [ ] Ada tes untuk parsing/state transition dan build target kernel berhasil.
- [ ] Dokumentasi menjelaskan asumsi hardware dan fitur yang belum didukung.
