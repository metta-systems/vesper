//! A stand-in for EL0 memory in the nucleus tests, which run without
//! translation: [`ArchObjects::copy_to_user`](crate::objects::ArchObjects::copy_to_user)
//! and `copy_from_user` reach this window instead of walking page tables.

use core::cell::UnsafeCell;

/// Bytes of the window.
const BYTES: usize = 8192;

struct Window {
    base: u64,
    length: usize,
    writable: bool,
    bytes: [u8; BYTES],
}

struct Shared(UnsafeCell<Window>);

// SAFETY: the nucleus tests run on one core, one test at a time, and no
// reference into the cell outlives a single call.
unsafe impl Sync for Shared {}

static WINDOW: Shared = Shared(UnsafeCell::new(Window {
    base: 0,
    length: 0,
    writable: false,
    bytes: [0; BYTES],
}));

/// EL0 memory for the nucleus tests: one window of up to `BYTES` bytes at a
/// chosen virtual address, readable, and writable or not.
pub struct FakeUserMemory;

impl FakeUserMemory {
    /// Map a fresh, zeroed window of `length` bytes at `base`. A length of
    /// zero maps nothing.
    pub fn map(base: u64, length: usize, writable: bool) {
        assert!(length <= BYTES, "the fake EL0 window holds {BYTES} bytes");
        // SAFETY: see `Shared`.
        let window = unsafe { &mut *WINDOW.0.get() };
        window.base = base;
        window.length = length;
        window.writable = writable;
        window.bytes = [0; BYTES];
    }

    /// The window's range for `length` bytes at `vaddr`, if inside it.
    fn range(window: &Window, vaddr: u64, length: usize) -> Option<core::ops::Range<usize>> {
        let start = usize::try_from(vaddr.checked_sub(window.base)?).ok()?;
        let end = start.checked_add(length)?;
        (end <= window.length).then_some(start..end)
    }

    pub fn write(vaddr: u64, bytes: &[u8]) -> bool {
        // SAFETY: see `Shared`.
        let window = unsafe { &mut *WINDOW.0.get() };
        let Some(range) = Self::range(window, vaddr, bytes.len()) else {
            return false;
        };
        if !window.writable {
            return false;
        }
        window.bytes[range].copy_from_slice(bytes);
        true
    }

    pub fn read(vaddr: u64, bytes: &mut [u8]) -> bool {
        // SAFETY: see `Shared`.
        let window = unsafe { &*WINDOW.0.get() };
        let Some(range) = Self::range(window, vaddr, bytes.len()) else {
            return false;
        };
        bytes.copy_from_slice(&window.bytes[range]);
        true
    }

    /// Make the window read-only (or writable again), as a handler that
    /// changed its own mappings would.
    pub fn set_writable(writable: bool) {
        // SAFETY: see `Shared`.
        unsafe { (*WINDOW.0.get()).writable = writable };
    }
}
