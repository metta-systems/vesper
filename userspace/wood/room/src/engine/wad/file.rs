//! Rust port of vendor/doomgeneric/w_file.c.
//!
//! Backing-store interface for WAD files. The original chocolate-doom
//! defines a `wad_file_class_t` vtable with three operations -
//! `W_StdC_OpenFile`, `W_StdC_CloseFile` and `W_StdC_Read` - and
//! optionally swaps in a `posix_wad_file` mmap-based class when
//! `-mmap` is passed and `HAVE_MMAP` is defined at build time. The
//! Rust port only ships the stdio-style backend, backed by
//! [`std::fs::File`], so the vtable and the `stdc_wad_file_t` subclass
//! collapse into the single concrete [`wad_file_t`]; `-mmap` is ignored.
//! Behaviour is identical to a chocolate-doom build without `HAVE_MMAP`.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::{
    ffi::c_void,
    fs::File,
    io::{Read, Seek, SeekFrom},
    ptr,
};

use crate::engine::zone::{PU_STATIC, Z_Free, Z_Malloc};

/// An open WAD file. Merges the C `wad_file_t` base and its
/// `stdc_wad_file_t` stdio backend.
pub struct wad_file_t {
    /// Pointer to an in-memory image of the whole WAD, or null when lumps
    /// are read from `fstream`. When set, `W_CacheLumpNum` returns
    /// pointers into it instead of reading lumps into the zone cache.
    pub mapped: *mut u8,
    /// Total file size in bytes.
    pub length: u32,
    /// The open WAD file.
    fstream: File,
}

/// Open a WAD file by path, returning a descriptor allocated in the static
/// zone heap, or null if the file cannot be opened or the allocation
/// fails. The C original optionally tried a series of `wad_file_classes`
/// based on the `-mmap` command-line argument; this port has only the
/// stdio backend (`W_StdC_OpenFile`).
///
/// The returned pointer (if non-null) is owned by the caller and must
/// eventually be passed to [`W_CloseFile`].
pub fn W_OpenFile(path: &str) -> *mut wad_file_t {
    let Ok(fstream) = File::open(path) else {
        return ptr::null_mut();
    };
    let length = fstream.metadata().map_or(0, |metadata| metadata.len());

    unsafe {
        let result = Z_Malloc(
            std::mem::size_of::<wad_file_t>() as i32,
            PU_STATIC,
            ptr::null_mut(),
        ) as *mut wad_file_t;

        if result.is_null() {
            return ptr::null_mut();
        }

        // The zone block is uninitialised, so write the fields in place.
        ptr::write(
            result,
            wad_file_t {
                mapped: ptr::null_mut(),
                length: length as u32,
                fstream,
            },
        );

        result
    }
}

/// Close the WAD file and free the descriptor. The descriptor must have
/// been returned by [`W_OpenFile`].
pub fn W_CloseFile(wad: *mut wad_file_t) {
    unsafe {
        ptr::drop_in_place(wad);
        Z_Free(wad as *mut c_void);
    }
}

/// Read up to `buffer_len` bytes from `offset` in the given WAD into
/// `buffer`, as `fread` does. Returns the number of bytes actually
/// transferred (which may be less than `buffer_len` at end of file or on
/// error).
///
/// `wad` must have been returned by [`W_OpenFile`]; `buffer` must point to
/// writable memory of at least `buffer_len` bytes.
pub fn W_Read(wad: *mut wad_file_t, offset: u32, buffer: *mut c_void, buffer_len: usize) -> usize {
    let file = unsafe { &mut (*wad).fstream };
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer as *mut u8, buffer_len) };
    if file.seek(SeekFrom::Start(offset.into())).is_err() {
        return 0;
    }
    let mut transferred = 0;
    while transferred < buffer_len {
        match file.read(&mut buffer[transferred..]) {
            Ok(0) => break,
            Ok(count) => transferred += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    transferred
}

#[cfg(test)]
impl wad_file_t {
    /// A descriptor over an arbitrary readable file with `mapped` set, for
    /// tests that only need the memory-mapped branches of the lump code.
    pub fn mapped_for_test(mapped: *mut u8) -> Self {
        wad_file_t {
            mapped,
            length: 0,
            fstream: File::open(env!("CARGO_MANIFEST_DIR").to_owned() + "/Cargo.toml")
                .expect("Cargo.toml is readable"),
        }
    }
}
