//! Re-export primitif sinkronisasi yang dipakai seluruh kernel.
//!
//! Gunakan tipe dari sini, bukan impor langsung dari `spin`, supaya
//! kalau kita ganti implementasi (mis. ke ticketlock) cukup ubah di sini.

pub use spin::{Mutex, MutexGuard, Once, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Wrapper `Mutex` yang aman untuk data yang diinisialisasi sekali saat boot
/// dan setelahnya hanya dibaca.
///
/// Berbeda dengan `Once<T>`, ini membungkus nilai yang bisa di-mutate setelah
/// inisialisasi (tapi jarang) -- berguna untuk state driver yang kadang diupdate.
pub struct BootMutex<T>(Mutex<Option<T>>);

impl<T> BootMutex<T> {
    /// Buat instance kosong (belum diinisialisasi).
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }

    /// Set nilai. Harus dipanggil tepat sekali saat boot, sebelum ada yang `get`.
    pub fn init(&self, value: T) {
        let mut guard = self.0.lock();
        *guard = Some(value);
    }

    /// Panggil closure dengan referensi ke nilai. Panic kalau belum diinisialisasi.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let guard = self.0.lock();
        f(guard.as_ref().expect("BootMutex: diakses sebelum init()"))
    }

    /// Panggil closure dengan referensi mutable ke nilai. Panic kalau belum diinisialisasi.
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let mut guard = self.0.lock();
        f(guard.as_mut().expect("BootMutex: diakses sebelum init()"))
    }

    /// Cek apakah sudah diinisialisasi.
    pub fn is_initialized(&self) -> bool {
        self.0.lock().is_some()
    }
}
