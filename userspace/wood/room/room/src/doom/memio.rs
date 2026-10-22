//! Rust port of vendor/doomgeneric/memio.c (companion header memio.h).
//!
//! In-memory replacement for the `stdio.h` `FILE *` family: a `MEMFILE` wraps
//! either a caller-provided read-only buffer or a self-growing write buffer,
//! and exposes `mem_fread` / `mem_fwrite` / `mem_fseek` / `mem_ftell` /
//! `mem_fclose` over it. Used by the MUS/MIDI conversion code and by
//! `dehacked` parsing where the input lives entirely in memory.
//!
//! Two intentional Rust-vs-C differences:
//! * `mem_fread` and `mem_fwrite` return `0` on a wrong-mode stream, while
//!   the C versions return `(size_t)-1` (i.e. `SIZE_MAX`). Callers in the
//!   tree only check for "fewer items than requested", so returning `0` is
//!   safer and avoids the implicit unsigned wraparound.
//! * `mem_fopen_read` zero-initialises the `alloced` field; the C version
//!   leaves it uninitialised because read streams never grow.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{c_int, c_void};

#[cfg(not(test))]
use crate::doom::z_zone::{Z_Free, Z_Malloc, PU_STATIC};

/// Direction of a `MEMFILE`: either a read view over an external buffer or a
/// write sink that owns its own growable buffer. Mirrors the unnamed C
/// `enum memfile_mode_t`.
#[repr(C)]
pub enum memfile_mode_t {
    /// Read mode: `buf` points at caller-owned data, `alloced` is unused.
    MODE_READ = 0,
    /// Write mode: `buf` is owned by the `MEMFILE` and grows on demand.
    MODE_WRITE = 1,
}

/// Origin selector for `mem_fseek`, mirroring `stdio.h` `SEEK_SET` /
/// `SEEK_CUR` / `SEEK_END`. Direct port of C `mem_rel_t`.
#[repr(C)]
pub enum mem_rel_t {
    /// Seek relative to the start of the buffer.
    MEM_SEEK_SET = 0,
    /// Seek relative to the current position.
    MEM_SEEK_CUR = 1,
    /// Seek relative to the end (i.e. `buflen`).
    MEM_SEEK_END = 2,
}

/// In-memory file handle, the port of C `struct _MEMFILE` (typedef
/// `MEMFILE`).
///
/// `#[repr(C)]` so the still-C callers of `mem_fopen_*` can read fields
/// directly if needed. Field semantics depend on `mode`:
/// * read mode -- `buf` and `buflen` describe a borrowed buffer; `alloced`
///   is unused.
/// * write mode -- `buf` is an owned allocation of `alloced` bytes; `buflen`
///   is the highest position ever written to (i.e. the logical file size).
#[repr(C)]
pub struct _MEMFILE {
    /// Pointer to the data buffer (borrowed in read mode, owned in write
    /// mode).
    pub buf: *mut u8,
    /// Logical file size in bytes; for read mode this is the size of the
    /// borrowed buffer.
    pub buflen: usize,
    /// Allocated capacity of `buf` in write mode (0 in read mode).
    pub alloced: usize,
    /// Current read/write cursor, in bytes from the start of `buf`.
    pub position: u32,
    /// Whether this handle is for reading or writing.
    pub mode: memfile_mode_t,
}

/// Test-only allocator: uses the global allocator so leak checkers and
/// `cargo test` work without bringing up the Doom zone allocator.
#[cfg(test)]
fn mem_malloc(size: usize) -> *mut c_void {
    let layout = std::alloc::Layout::from_size_align(size, 1).unwrap();
    unsafe { std::alloc::alloc(layout) as *mut c_void }
}

/// Test-only deallocator paired with the test-mode `mem_malloc`.
///
/// Uses a `(1, 1)` placeholder layout because the original size is not
/// tracked; this matches what the global allocator on the supported
/// platforms (jemalloc, system malloc) tolerates.
#[cfg(test)]
fn mem_free(ptr: *mut c_void) {
    if !ptr.is_null() {
        let layout = std::alloc::Layout::from_size_align(1, 1).unwrap();
        unsafe { std::alloc::dealloc(ptr as *mut u8, layout) };
    }
}

/// Production allocator: routes through Doom's zone allocator at
/// `PU_STATIC` priority, matching the C source's use of `Z_Malloc`.
#[cfg(not(test))]
fn mem_malloc(size: usize) -> *mut c_void {
    unsafe { Z_Malloc(size as c_int, PU_STATIC, std::ptr::null_mut()) }
}

/// Production deallocator: forwards to `Z_Free`, matching the C source.
#[cfg(not(test))]
fn mem_free(ptr: *mut c_void) {
    unsafe { Z_Free(ptr) }
}

/// Test-only zero-initialised allocator.
#[cfg(test)]
fn mem_malloc_zero(size: usize) -> *mut c_void {
    let layout = std::alloc::Layout::from_size_align(size, 1).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    ptr as *mut c_void
}

/// Production zero-initialised allocator: allocates via the zone allocator
/// and explicitly writes zero bytes, because `Z_Malloc` does not clear
/// memory.
#[cfg(not(test))]
fn mem_malloc_zero(size: usize) -> *mut c_void {
    let ptr = mem_malloc(size);
    if !ptr.is_null() {
        unsafe { std::ptr::write_bytes(ptr as *mut u8, 0, size) };
    }
    ptr
}

/// Opens a read-only `MEMFILE` over a caller-owned buffer.
///
/// The buffer is borrowed; `mem_fclose` does not free it. The C signature
/// uses `void *` for parity with `fread`; the caller must keep `buf` alive
/// until `mem_fclose` is called. Exported with C linkage for callers in
/// `mus2mid.c` and `deh_io.c`.
#[no_mangle]
pub extern "C" fn mem_fopen_read(buf: *mut c_void, buflen: usize) -> *mut _MEMFILE {
    let file: *mut _MEMFILE = mem_malloc(std::mem::size_of::<_MEMFILE>()) as *mut _MEMFILE;
    unsafe {
        (*file).buf = buf as *mut u8;
        (*file).buflen = buflen;
        (*file).alloced = 0;
        (*file).position = 0;
        (*file).mode = memfile_mode_t::MODE_READ;
    }
    file
}

/// Reads `nmemb` records of `size` bytes from `stream` into `buf`, returning
/// the number of complete records read.
///
/// Truncates to whole records when fewer than `size * nmemb` bytes remain in
/// the stream. Returns 0 if the stream is not a read stream (the C source
/// returns `(size_t)-1` here and prints a diagnostic; the Rust port returns
/// 0, which the callers in this tree handle correctly without the wrap-around
/// surprise of `SIZE_MAX`).
#[no_mangle]
pub extern "C" fn mem_fread(
    buf: *mut c_void,
    size: usize,
    nmemb: usize,
    stream: *mut _MEMFILE,
) -> usize {
    let stream = unsafe { &mut *stream };

    if !matches!(stream.mode, memfile_mode_t::MODE_READ) {
        return 0;
    }

    let mut items = nmemb;

    if items * size > stream.buflen - stream.position as usize {
        items = (stream.buflen - stream.position as usize) / size;
    }

    unsafe {
        std::ptr::copy_nonoverlapping(
            stream.buf.add(stream.position as usize),
            buf as *mut u8,
            items * size,
        );
    }

    stream.position += (items * size) as u32;

    items
}

/// Opens a write-mode `MEMFILE` with a fresh 1 KiB initial allocation.
///
/// The backing buffer doubles each time `mem_fwrite` would overflow it.
/// Use `mem_get_buf` to obtain the final buffer pointer and length, and
/// `mem_fclose` to release the allocation. Exported with C linkage for use
/// from `mus2mid.c`.
#[no_mangle]
pub extern "C" fn mem_fopen_write() -> *mut _MEMFILE {
    let file: *mut _MEMFILE = mem_malloc(std::mem::size_of::<_MEMFILE>()) as *mut _MEMFILE;
    let initial_alloc = 1024usize;
    let buf = mem_malloc(initial_alloc) as *mut u8;

    unsafe {
        (*file).alloced = initial_alloc;
        (*file).buf = buf;
        (*file).buflen = 0;
        (*file).position = 0;
        (*file).mode = memfile_mode_t::MODE_WRITE;
    }
    file
}

/// Writes `nmemb` records of `size` bytes from `ptr` into `stream`, growing
/// the backing buffer as needed (doubling each time).
///
/// Returns `nmemb` on success, or 0 if the stream is not a write stream.
/// The C version returns `(size_t)-1` for the wrong-mode case; the Rust port
/// returns 0 for the same defensive-coding reasons noted on `mem_fread`.
/// Updates `stream.buflen` to the new high-water mark.
#[no_mangle]
pub extern "C" fn mem_fwrite(
    ptr: *const c_void,
    size: usize,
    nmemb: usize,
    stream: *mut _MEMFILE,
) -> usize {
    let stream = unsafe { &mut *stream };

    if !matches!(stream.mode, memfile_mode_t::MODE_WRITE) {
        return 0;
    }

    let bytes = size * nmemb;

    while bytes > stream.alloced - stream.position as usize {
        let new_alloc = stream.alloced * 2;
        let newbuf = mem_malloc(new_alloc) as *mut u8;
        unsafe {
            std::ptr::copy_nonoverlapping(stream.buf, newbuf, stream.alloced);
        }
        mem_free(stream.buf as *mut c_void);
        stream.buf = newbuf;
        stream.alloced = new_alloc;
    }

    unsafe {
        std::ptr::copy_nonoverlapping(
            ptr as *const u8,
            stream.buf.add(stream.position as usize),
            bytes,
        );
    }
    stream.position += bytes as u32;

    if stream.position as usize > stream.buflen {
        stream.buflen = stream.position as usize;
    }

    nmemb
}

/// Reports the current buffer pointer and logical size through the
/// out-parameters.
///
/// Useful on a write-mode stream once all data has been written: the
/// returned pointer remains valid until the next `mem_fwrite` (which may
/// reallocate) or `mem_fclose`.
#[no_mangle]
pub extern "C" fn mem_get_buf(stream: *mut _MEMFILE, buf: *mut *mut c_void, buflen: *mut usize) {
    let stream = unsafe { &*stream };
    unsafe {
        *buf = stream.buf as *mut c_void;
        *buflen = stream.buflen;
    }
}

/// Releases a `MEMFILE`.
///
/// In write mode also frees the owned data buffer; in read mode the borrowed
/// buffer is left to the caller. After this call the `stream` pointer is
/// dangling.
#[no_mangle]
pub extern "C" fn mem_fclose(stream: *mut _MEMFILE) {
    let stream = unsafe { &*stream };

    if matches!(stream.mode, memfile_mode_t::MODE_WRITE) {
        mem_free(stream.buf as *mut c_void);
    }

    mem_free(stream as *const _MEMFILE as *mut c_void);
}

/// Returns the current read/write position, like `ftell`.
///
/// The return type is `c_int` for parity with the C signature (`long` in C,
/// but the existing callers only use small offsets).
#[no_mangle]
pub extern "C" fn mem_ftell(stream: *mut _MEMFILE) -> c_int {
    let stream = unsafe { &*stream };
    stream.position as c_int
}

/// Repositions the cursor relative to `whence`.
///
/// Returns 0 on success, -1 if the resulting position is not strictly less
/// than `buflen` (i.e. seeking *to* the end of the buffer is rejected,
/// because the buffer is read-only at that point and the cursor would
/// describe an unreadable byte). Matches the C source byte-for-byte; in
/// particular it suffers the same defects: `MEM_SEEK_CUR` and
/// `MEM_SEEK_END` use 32-bit unsigned arithmetic and do not detect under-
/// or overflow if a negative offset would seek past 0.
#[no_mangle]
pub extern "C" fn mem_fseek(stream: *mut _MEMFILE, position: c_int, whence: mem_rel_t) -> c_int {
    let stream = unsafe { &mut *stream };

    let newpos = match whence {
        mem_rel_t::MEM_SEEK_SET => position as u32,
        mem_rel_t::MEM_SEEK_CUR => stream.position.wrapping_add(position as u32),
        mem_rel_t::MEM_SEEK_END => (stream.buflen as c_int + position) as u32,
    };

    if (newpos as usize) < stream.buflen {
        stream.position = newpos;
        0
    } else {
        -1
    }
}

/// Unit tests covering round-trip read/write, seek semantics, mode errors,
/// buffer growth, and partial-element reads. Use the global allocator
/// stand-ins for `mem_malloc` / `mem_free` so the Doom zone allocator does
/// not need to be initialised.
#[cfg(test)]
mod tests {
    use super::*;

    /// Round-tripping a four-byte payload through a write stream and a
    /// read stream over the same buffer must yield the original bytes.
    #[test]
    fn round_trip_small_write() {
        let write_file = mem_fopen_write();
        let data: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

        mem_fwrite(data.as_ptr() as *const c_void, 1, 4, write_file);

        let mut buf_ptr: *mut c_void = std::ptr::null_mut();
        let mut buflen: usize = 0;
        mem_get_buf(write_file, &mut buf_ptr, &mut buflen);

        assert_eq!(buflen, 4);

        let read_file = mem_fopen_read(buf_ptr, buflen);
        let mut read_buf = [0u8; 4];

        let items_read = mem_fread(read_buf.as_mut_ptr() as *mut c_void, 1, 4, read_file);

        assert_eq!(items_read, 4);
        assert_eq!(read_buf, data);

        mem_fclose(read_file);
        mem_fclose(write_file);
    }

    /// Exercises each `mem_rel_t` variant in turn and verifies that
    /// `mem_ftell` and a follow-up `mem_fread` see the expected position.
    #[test]
    fn seek_set_cur_end() {
        let write_file = mem_fopen_write();
        let data: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

        mem_fwrite(data.as_ptr() as *const c_void, 1, 8, write_file);

        let mut buf_ptr: *mut c_void = std::ptr::null_mut();
        let mut buflen: usize = 0;
        mem_get_buf(write_file, &mut buf_ptr, &mut buflen);

        let read_file = mem_fopen_read(buf_ptr, buflen);

        let mut buf = [0u8; 2];

        assert_eq!(mem_fseek(read_file, 4, mem_rel_t::MEM_SEEK_SET), 0);
        assert_eq!(mem_ftell(read_file), 4);
        assert_eq!(
            mem_fread(buf.as_mut_ptr() as *mut c_void, 1, 2, read_file),
            2
        );
        assert_eq!(buf, [5, 6]);

        assert_eq!(mem_fseek(read_file, -3, mem_rel_t::MEM_SEEK_CUR), 0);
        assert_eq!(mem_ftell(read_file), 3);
        assert_eq!(
            mem_fread(buf.as_mut_ptr() as *mut c_void, 1, 2, read_file),
            2
        );
        assert_eq!(buf, [4, 5]);

        assert_eq!(mem_fseek(read_file, -2, mem_rel_t::MEM_SEEK_END), 0);
        assert_eq!(mem_ftell(read_file), 6);
        assert_eq!(
            mem_fread(buf.as_mut_ptr() as *mut c_void, 1, 2, read_file),
            2
        );
        assert_eq!(buf, [7, 8]);

        mem_fclose(read_file);
        mem_fclose(write_file);
    }

    /// Reading from a write-mode stream must return 0 items.
    #[test]
    fn fread_on_write_mode_returns_zero() {
        let write_file = mem_fopen_write();
        let data = [0u8; 4];
        mem_fwrite(data.as_ptr() as *const c_void, 1, 4, write_file);

        let mut out = [0u8; 4];
        let items = mem_fread(out.as_mut_ptr() as *mut c_void, 1, 4, write_file);
        assert_eq!(items, 0, "fread on write-mode stream must return 0");

        mem_fclose(write_file);
    }

    /// Writing to a read-mode stream must return 0 items.
    #[test]
    fn fwrite_on_read_mode_returns_zero() {
        // Create a small backing buffer on the stack.
        let mut backing = [1u8, 2, 3, 4];
        let read_file = mem_fopen_read(backing.as_mut_ptr() as *mut c_void, backing.len());

        let payload = [0xFFu8; 4];
        let items = mem_fwrite(payload.as_ptr() as *const c_void, 1, 4, read_file);
        assert_eq!(items, 0, "fwrite on read-mode stream must return 0");

        mem_fclose(read_file);
    }

    /// Seeking exactly to the buffer length (== one past the last byte) must
    /// return -1 because that position is out of range for reading.
    #[test]
    fn fseek_to_exact_end_returns_neg1() {
        let write_file = mem_fopen_write();
        let data = [0u8; 8];
        mem_fwrite(data.as_ptr() as *const c_void, 1, 8, write_file);

        let mut buf_ptr: *mut c_void = std::ptr::null_mut();
        let mut buflen: usize = 0;
        mem_get_buf(write_file, &mut buf_ptr, &mut buflen);

        let read_file = mem_fopen_read(buf_ptr, buflen);
        // Seeking to buflen (8) is past the last valid position (7).
        assert_eq!(
            mem_fseek(read_file, 8, mem_rel_t::MEM_SEEK_SET),
            -1,
            "seek to exact buflen should fail"
        );

        mem_fclose(read_file);
        mem_fclose(write_file);
    }

    /// Seeking far beyond the buffer must also return -1.
    #[test]
    fn fseek_beyond_end_returns_neg1() {
        let write_file = mem_fopen_write();
        let data = [0u8; 4];
        mem_fwrite(data.as_ptr() as *const c_void, 1, 4, write_file);

        let mut buf_ptr: *mut c_void = std::ptr::null_mut();
        let mut buflen: usize = 0;
        mem_get_buf(write_file, &mut buf_ptr, &mut buflen);

        let read_file = mem_fopen_read(buf_ptr, buflen);
        assert_eq!(
            mem_fseek(read_file, 100, mem_rel_t::MEM_SEEK_SET),
            -1,
            "seek past end should return -1"
        );

        mem_fclose(read_file);
        mem_fclose(write_file);
    }

    /// Writing enough bytes to exceed the initial 1024-byte allocation forces
    /// the write buffer to grow via realloc.
    #[test]
    fn fwrite_grows_buffer_past_initial_alloc() {
        let write_file = mem_fopen_write();
        let data = vec![0xABu8; 2048]; // 2× initial allocation

        let items = mem_fwrite(data.as_ptr() as *const c_void, 1, data.len(), write_file);
        assert_eq!(items, data.len());

        let mut buf_ptr: *mut c_void = std::ptr::null_mut();
        let mut buflen: usize = 0;
        mem_get_buf(write_file, &mut buf_ptr, &mut buflen);
        assert_eq!(buflen, 2048);

        // Verify all bytes round-trip correctly.
        let read_file = mem_fopen_read(buf_ptr, buflen);
        let mut out = vec![0u8; 2048];
        let items_read = mem_fread(out.as_mut_ptr() as *mut c_void, 1, out.len(), read_file);
        assert_eq!(items_read, 2048);
        assert_eq!(out, data);

        mem_fclose(read_file);
        mem_fclose(write_file);
    }

    /// mem_fread with a multi-byte element size reads only whole elements.
    #[test]
    fn fread_partial_elements_truncated() {
        let write_file = mem_fopen_write();
        let data = [1u8, 2, 3, 4, 5]; // 5 bytes
        mem_fwrite(data.as_ptr() as *const c_void, 1, 5, write_file);

        let mut buf_ptr: *mut c_void = std::ptr::null_mut();
        let mut buflen: usize = 0;
        mem_get_buf(write_file, &mut buf_ptr, &mut buflen);

        let read_file = mem_fopen_read(buf_ptr, buflen);
        let mut out = [0u8; 6];
        // request 3 items of size 2 = 6 bytes, but only 5 available → 2 items
        let items = mem_fread(out.as_mut_ptr() as *mut c_void, 2, 3, read_file);
        assert_eq!(items, 2, "only 2 complete 2-byte items fit in 5 bytes");
        assert_eq!(out[0..4], [1, 2, 3, 4]);

        mem_fclose(read_file);
        mem_fclose(write_file);
    }
}
