extern "C" {
    pub type _IO_wide_data;
    pub type _IO_codecvt;
    pub type _IO_marker;
    fn fclose(__stream: *mut FILE) -> ::core::ffi::c_int;
    fn fopen(
        __filename: *const ::core::ffi::c_char,
        __modes: *const ::core::ffi::c_char,
    ) -> *mut FILE;
    fn fread(
        __ptr: *mut ::core::ffi::c_void,
        __size: size_t,
        __n: size_t,
        __stream: *mut FILE,
    ) -> ::core::ffi::c_ulong;
    fn fseek(
        __stream: *mut FILE,
        __off: ::core::ffi::c_long,
        __whence: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int;
    fn M_FileLength(handle: *mut FILE) -> ::core::ffi::c_long;
    fn Z_Malloc(
        size: ::core::ffi::c_int,
        tag: ::core::ffi::c_int,
        ptr: *mut ::core::ffi::c_void,
    ) -> *mut ::core::ffi::c_void;
    fn Z_Free(ptr: *mut ::core::ffi::c_void);
}
pub type size_t = usize;
pub type __uint8_t = u8;
pub type __off_t = ::core::ffi::c_long;
pub type __off64_t = ::core::ffi::c_long;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct _IO_FILE {
    pub _flags: ::core::ffi::c_int,
    pub _IO_read_ptr: *mut ::core::ffi::c_char,
    pub _IO_read_end: *mut ::core::ffi::c_char,
    pub _IO_read_base: *mut ::core::ffi::c_char,
    pub _IO_write_base: *mut ::core::ffi::c_char,
    pub _IO_write_ptr: *mut ::core::ffi::c_char,
    pub _IO_write_end: *mut ::core::ffi::c_char,
    pub _IO_buf_base: *mut ::core::ffi::c_char,
    pub _IO_buf_end: *mut ::core::ffi::c_char,
    pub _IO_save_base: *mut ::core::ffi::c_char,
    pub _IO_backup_base: *mut ::core::ffi::c_char,
    pub _IO_save_end: *mut ::core::ffi::c_char,
    pub _markers: *mut _IO_marker,
    pub _chain: *mut _IO_FILE,
    pub _fileno: ::core::ffi::c_int,
    pub _flags2: ::core::ffi::c_int,
    pub _old_offset: __off_t,
    pub _cur_column: ::core::ffi::c_ushort,
    pub _vtable_offset: ::core::ffi::c_schar,
    pub _shortbuf: [::core::ffi::c_char; 1],
    pub _lock: *mut ::core::ffi::c_void,
    pub _offset: __off64_t,
    pub _codecvt: *mut _IO_codecvt,
    pub _wide_data: *mut _IO_wide_data,
    pub _freeres_list: *mut _IO_FILE,
    pub _freeres_buf: *mut ::core::ffi::c_void,
    pub __pad5: size_t,
    pub _mode: ::core::ffi::c_int,
    pub _unused2: [::core::ffi::c_char; 20],
}
pub type _IO_lock_t = ();
pub type FILE = _IO_FILE;
pub type uint8_t = __uint8_t;
pub type byte = uint8_t;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct _wad_file_s {
    pub file_class: *mut wad_file_class_t,
    pub mapped: *mut byte,
    pub length: ::core::ffi::c_uint,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct wad_file_class_t {
    pub OpenFile: Option<unsafe extern "C" fn(*mut ::core::ffi::c_char) -> *mut wad_file_t>,
    pub CloseFile: Option<unsafe extern "C" fn(*mut wad_file_t) -> ()>,
    pub Read: Option<
        unsafe extern "C" fn(
            *mut wad_file_t,
            ::core::ffi::c_uint,
            *mut ::core::ffi::c_void,
            size_t,
        ) -> size_t,
    >,
}
pub type wad_file_t = _wad_file_s;
pub type C2Rust_Unnamed = ::core::ffi::c_uint;
pub const PU_NUM_TAGS: C2Rust_Unnamed = 9;
pub const PU_CACHE: C2Rust_Unnamed = 8;
pub const PU_PURGELEVEL: C2Rust_Unnamed = 7;
pub const PU_LEVSPEC: C2Rust_Unnamed = 6;
pub const PU_LEVEL: C2Rust_Unnamed = 5;
pub const PU_FREE: C2Rust_Unnamed = 4;
pub const PU_MUSIC: C2Rust_Unnamed = 3;
pub const PU_SOUND: C2Rust_Unnamed = 2;
pub const PU_STATIC: C2Rust_Unnamed = 1;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct stdc_wad_file_t {
    pub wad: wad_file_t,
    pub fstream: *mut FILE,
}
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const SEEK_SET: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
unsafe extern "C" fn W_StdC_OpenFile(mut path: *mut ::core::ffi::c_char) -> *mut wad_file_t {
    let mut result: *mut stdc_wad_file_t = ::core::ptr::null_mut::<stdc_wad_file_t>();
    let mut fstream: *mut FILE = ::core::ptr::null_mut::<FILE>();
    fstream = fopen(path, b"rb\0".as_ptr() as *const ::core::ffi::c_char) as *mut FILE;
    if fstream.is_null() {
        return ::core::ptr::null_mut::<wad_file_t>();
    }
    result = Z_Malloc(
        ::core::mem::size_of::<stdc_wad_file_t>() as ::core::ffi::c_int,
        PU_STATIC as ::core::ffi::c_int,
        ::core::ptr::null_mut::<::core::ffi::c_void>(),
    ) as *mut stdc_wad_file_t;
    (*result).wad.file_class = &raw mut stdc_wad_file;
    (*result).wad.mapped = ::core::ptr::null_mut::<byte>();
    (*result).wad.length = M_FileLength(fstream) as ::core::ffi::c_uint;
    (*result).fstream = fstream;
    return &raw mut (*result).wad;
}
unsafe extern "C" fn W_StdC_CloseFile(mut wad: *mut wad_file_t) {
    let mut stdc_wad: *mut stdc_wad_file_t = ::core::ptr::null_mut::<stdc_wad_file_t>();
    stdc_wad = wad as *mut stdc_wad_file_t;
    fclose((*stdc_wad).fstream);
    Z_Free(stdc_wad as *mut ::core::ffi::c_void);
}
#[no_mangle]
pub unsafe extern "C" fn W_StdC_Read(
    mut wad: *mut wad_file_t,
    mut offset: ::core::ffi::c_uint,
    mut buffer: *mut ::core::ffi::c_void,
    mut buffer_len: size_t,
) -> size_t {
    let mut stdc_wad: *mut stdc_wad_file_t = ::core::ptr::null_mut::<stdc_wad_file_t>();
    let mut result: size_t = 0;
    stdc_wad = wad as *mut stdc_wad_file_t;
    fseek((*stdc_wad).fstream, offset as ::core::ffi::c_long, SEEK_SET);
    result = fread(buffer, 1 as size_t, buffer_len, (*stdc_wad).fstream) as size_t;
    return result;
}
#[no_mangle]
pub static mut stdc_wad_file: wad_file_class_t = wad_file_class_t {
    OpenFile: Some(
        W_StdC_OpenFile as unsafe extern "C" fn(*mut ::core::ffi::c_char) -> *mut wad_file_t,
    ),
    CloseFile: Some(W_StdC_CloseFile as unsafe extern "C" fn(*mut wad_file_t) -> ()),
    Read: Some(
        W_StdC_Read
            as unsafe extern "C" fn(
                *mut wad_file_t,
                ::core::ffi::c_uint,
                *mut ::core::ffi::c_void,
                size_t,
            ) -> size_t,
    ),
};
