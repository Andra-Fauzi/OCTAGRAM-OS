# Grafis dan terminal

Grafis kernel sengaja minimal: satu framebuffer linier dari Limine dan terminal
berbasis sel karakter. Tidak ada compositor, akselerasi GPU, atau API gambar
untuk program user.

## Framebuffer

`kernel/src/graphics/mod.rs` menyediakan singleton `Framebuffer`:

- `get()` → `Option<Framebuffer>` dari `FramebufferRequest`;
- `width()`, `height()`, `pitch()`, `addr()`;
- `write_pixel()` (bounds-checked), `write_pixel_unchecked()` (untuk hot path),
  `fill_rect()`, dan `clear()`;
- `Color` di `graphics/color.rs` (format 32-bit, kanal 8-bit).

Asumsi: framebuffer 32 bit per pixel (BGRA/RGBA sesuai firmware). Tidak ada
double buffering atau vsync; menggambar langsung menulis ke memori framebuffer.

## Terminal

`kernel/src/terminal/mod.rs` mengimplementasikan empat virtual console:

| Properti | Nilai |
| --- | --- |
| Jumlah terminal | `TERMINAL_COUNT = 4` (`tty0`..`tty3`) |
| Ukuran sel | 8 × 16 pixel (`FONT8X16`) |
| Grid maksimum | 160 kolom × 60 baris |
| Grid default tanpa framebuffer | 80 × 25 |

Fitur:

- output per terminal (`terminal_print_to`), terminal aktif, dan switch dengan
  Ctrl+Alt+F1..F4;
- scroll otomatis saat baris penuh, `terminal_clear`, `terminal_backspace`;
- penulisan karakter langsung pada koordinat sel untuk layar editor;
- `show_kernel_exception()` menampilkan vektor, error code, RIP, dan alamat
  fault di layar saat terjadi exception;
- `redraw_pending()` menggambar ulang grid ke framebuffer setelah perubahan.

Font bitmap 8×16 ada di `terminal/font.rs`. Tidak ada dukungan font vektor,
Unicode lebar ganda, atau ligatur; karakter non-ASCII yang tersedia terbatas pada
glyph di font tersebut.

## Input

- Keyboard PS/2 (`drivers/keyboard.rs`) dan keyboard USB HID sama-sama
  menghasilkan event karakter/keycode untuk terminal aktif.
- Shell membaca input baris dengan echo, Backspace, Enter, Ctrl-D, dan Ctrl-C.
- Karena framebuffer tidak punya kursor hardware, kursor digambar oleh terminal.

## Program user

`fb_demo` adalah contoh program user yang meminta informasi framebuffer lewat
syscall; userland tidak memiliki syscall untuk menggambar langsung ke
framebuffer. Semua output program user melewati standard output (TTY), bukan
grafik.

## Batasan

- Tanpa window system, compositor, atau clipboard.
- Tanpa output grafis untuk program user; `fb_demo` hanya demonstrasi informasi.
- Tanpa dukungan mode grafis ganda, resolusi dinamis, atau hotplug display.
- Tanpa akselerasi 2D/3D atau driver GPU.
