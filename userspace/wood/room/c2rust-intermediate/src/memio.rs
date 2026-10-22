extern "C" {
    fn printf(__format: *const ::core::ffi::c_char, ...) -> ::core::ffi::c_int;
    fn memcpy(
        __dest: *mut ::core::ffi::c_void,
        __src: *const ::core::ffi::c_void,
        __n: size_t,
    ) -> *mut ::core::ffi::c_void;
    fn Z_Malloc(
        size: ::core::ffi::c_int,
        tag: ::core::ffi::c_int,
        ptr: *mut ::core::ffi::c_void,
    ) -> *mut ::core::ffi::c_void;
    fn Z_Free(ptr: *mut ::core::ffi::c_void);
}
pub type size_t = usize;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct _MEMFILE {
    pub buf: *mut ::core::ffi::c_uchar,
    pub buflen: size_t,
    pub alloced: size_t,
    pub position: ::core::ffi::c_uint,
    pub mode: memfile_mode_t,
}
pub type memfile_mode_t = ::core::ffi::c_uint;
pub const MODE_WRITE: memfile_mode_t = 1;
pub const MODE_READ: memfile_mode_t = 0;
pub type MEMFILE = _MEMFILE;
pub type mem_rel_t = ::core::ffi::c_uint;
pub const MEM_SEEK_END: mem_rel_t = 2;
pub const MEM_SEEK_CUR: mem_rel_t = 1;
pub const MEM_SEEK_SET: mem_rel_t = 0;
pub const PU_STATIC: C2Rust_Unnamed = 1;
pub type C2Rust_Unnamed = ::core::ffi::c_uint;
pub const PU_NUM_TAGS: C2Rust_Unnamed = 9;
pub const PU_CACHE: C2Rust_Unnamed = 8;
pub const PU_PURGELEVEL: C2Rust_Unnamed = 7;
pub const PU_LEVSPEC: C2Rust_Unnamed = 6;
pub const PU_LEVEL: C2Rust_Unnamed = 5;
pub const PU_FREE: C2Rust_Unnamed = 4;
pub const PU_MUSIC: C2Rust_Unnamed = 3;
pub const PU_SOUND: C2Rust_Unnamed = 2;
#[no_mangle]
pub unsafe extern "C" fn mem_fopen_read(
    mut buf: *mut ::core::ffi::c_void,
    mut buflen: size_t,
) -> *mut MEMFILE {
    let mut file: *mut MEMFILE = ::core::ptr::null_mut::<MEMFILE>();
    file = Z_Malloc(
        ::core::mem::size_of::<MEMFILE>() as ::core::ffi::c_int,
        PU_STATIC as ::core::ffi::c_int,
        ::core::ptr::null_mut::<::core::ffi::c_void>(),
    ) as *mut MEMFILE;
    (*file).buf = buf as *mut ::core::ffi::c_uchar;
    (*file).buflen = buflen;
    (*file).position = 0 as ::core::ffi::c_uint;
    (*file).mode = MODE_READ;
    return file;
}
#[no_mangle]
pub unsafe extern "C" fn mem_fread(
    mut buf: *mut ::core::ffi::c_void,
    mut size: size_t,
    mut nmemb: size_t,
    mut stream: *mut MEMFILE,
) -> size_t {
    let mut items: size_t = 0;
    if (*stream).mode as ::core::ffi::c_uint
        != MODE_READ as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        printf(b"not a read stream\n\0".as_ptr() as *const ::core::ffi::c_char);
        return -1 as ::core::ffi::c_int as size_t;
    }
    items = nmemb;
    if items.wrapping_mul(size) > (*stream).buflen.wrapping_sub((*stream).position as size_t) {
        items = (*stream)
            .buflen
            .wrapping_sub((*stream).position as size_t)
            .wrapping_div(size);
    }
    memcpy(
        buf,
        (*stream).buf.offset((*stream).position as isize) as *const ::core::ffi::c_void,
        items.wrapping_mul(size),
    );
    (*stream).position = ((*stream).position as size_t).wrapping_add(items.wrapping_mul(size))
        as ::core::ffi::c_uint;
    return items;
}
#[no_mangle]
pub unsafe extern "C" fn mem_fopen_write() -> *mut MEMFILE {
    let mut file: *mut MEMFILE = ::core::ptr::null_mut::<MEMFILE>();
    file = Z_Malloc(
        ::core::mem::size_of::<MEMFILE>() as ::core::ffi::c_int,
        PU_STATIC as ::core::ffi::c_int,
        ::core::ptr::null_mut::<::core::ffi::c_void>(),
    ) as *mut MEMFILE;
    (*file).alloced = 1024 as size_t;
    (*file).buf = Z_Malloc(
        (*file).alloced as ::core::ffi::c_int,
        PU_STATIC as ::core::ffi::c_int,
        ::core::ptr::null_mut::<::core::ffi::c_void>(),
    ) as *mut ::core::ffi::c_uchar;
    (*file).buflen = 0 as size_t;
    (*file).position = 0 as ::core::ffi::c_uint;
    (*file).mode = MODE_WRITE;
    return file;
}
#[no_mangle]
pub unsafe extern "C" fn mem_fwrite(
    mut ptr: *const ::core::ffi::c_void,
    mut size: size_t,
    mut nmemb: size_t,
    mut stream: *mut MEMFILE,
) -> size_t {
    let mut bytes: size_t = 0;
    if (*stream).mode as ::core::ffi::c_uint
        != MODE_WRITE as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        return -1 as ::core::ffi::c_int as size_t;
    }
    bytes = size.wrapping_mul(nmemb);
    while bytes > (*stream).alloced.wrapping_sub((*stream).position as size_t) {
        let mut newbuf: *mut ::core::ffi::c_uchar = ::core::ptr::null_mut::<::core::ffi::c_uchar>();
        newbuf = Z_Malloc(
            (*stream).alloced.wrapping_mul(2 as size_t) as ::core::ffi::c_int,
            PU_STATIC as ::core::ffi::c_int,
            ::core::ptr::null_mut::<::core::ffi::c_void>(),
        ) as *mut ::core::ffi::c_uchar;
        memcpy(
            newbuf as *mut ::core::ffi::c_void,
            (*stream).buf as *const ::core::ffi::c_void,
            (*stream).alloced,
        );
        Z_Free((*stream).buf as *mut ::core::ffi::c_void);
        (*stream).buf = newbuf;
        (*stream).alloced = (*stream).alloced.wrapping_mul(2 as size_t);
    }
    memcpy(
        (*stream).buf.offset((*stream).position as isize) as *mut ::core::ffi::c_void,
        ptr,
        bytes,
    );
    (*stream).position = ((*stream).position as size_t).wrapping_add(bytes) as ::core::ffi::c_uint;
    if (*stream).position as size_t > (*stream).buflen {
        (*stream).buflen = (*stream).position as size_t;
    }
    return nmemb;
}
#[no_mangle]
pub unsafe extern "C" fn mem_get_buf(
    mut stream: *mut MEMFILE,
    mut buf: *mut *mut ::core::ffi::c_void,
    mut buflen: *mut size_t,
) {
    *buf = (*stream).buf as *mut ::core::ffi::c_void;
    *buflen = (*stream).buflen;
}
#[no_mangle]
pub unsafe extern "C" fn mem_fclose(mut stream: *mut MEMFILE) {
    if (*stream).mode as ::core::ffi::c_uint
        == MODE_WRITE as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        Z_Free((*stream).buf as *mut ::core::ffi::c_void);
    }
    Z_Free(stream as *mut ::core::ffi::c_void);
}
#[no_mangle]
pub unsafe extern "C" fn mem_ftell(mut stream: *mut MEMFILE) -> ::core::ffi::c_long {
    return (*stream).position as ::core::ffi::c_long;
}
#[no_mangle]
pub unsafe extern "C" fn mem_fseek(
    mut stream: *mut MEMFILE,
    mut position: ::core::ffi::c_long,
    mut whence: mem_rel_t,
) -> ::core::ffi::c_int {
    let mut newpos: ::core::ffi::c_uint = 0;
    match whence as ::core::ffi::c_uint {
        0 => {
            newpos = position as ::core::ffi::c_int as ::core::ffi::c_uint;
        }
        1 => {
            newpos = ((*stream).position as ::core::ffi::c_long + position) as ::core::ffi::c_int
                as ::core::ffi::c_uint;
        }
        2 => {
            newpos = (*stream).buflen.wrapping_add(position as size_t) as ::core::ffi::c_int
                as ::core::ffi::c_uint;
        }
        _ => return -1 as ::core::ffi::c_int,
    }
    if (newpos as size_t) < (*stream).buflen {
        (*stream).position = newpos;
        return 0 as ::core::ffi::c_int;
    } else {
        printf(
            b"Error seeking to %i\n\0".as_ptr() as *const ::core::ffi::c_char,
            newpos,
        );
        return -1 as ::core::ffi::c_int;
    };
}
