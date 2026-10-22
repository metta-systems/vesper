extern "C" {
    pub type _IO_wide_data;
    pub type _IO_codecvt;
    pub type _IO_marker;
    fn fprintf(
        __stream: *mut FILE,
        __format: *const ::core::ffi::c_char,
        ...
    ) -> ::core::ffi::c_int;
    fn printf(__format: *const ::core::ffi::c_char, ...) -> ::core::ffi::c_int;
    fn snprintf(
        __s: *mut ::core::ffi::c_char,
        __maxlen: size_t,
        __format: *const ::core::ffi::c_char,
        ...
    ) -> ::core::ffi::c_int;
    fn I_ZoneBase(size: *mut ::core::ffi::c_int) -> *mut byte;
    fn I_ErrorV(msg: *const ::core::ffi::c_char);
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
pub struct memzone_t {
    pub size: ::core::ffi::c_int,
    pub blocklist: memblock_t,
    pub rover: *mut memblock_t,
}
pub type memblock_t = memblock_s;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct memblock_s {
    pub size: ::core::ffi::c_int,
    pub user: *mut *mut ::core::ffi::c_void,
    pub tag: ::core::ffi::c_int,
    pub id: ::core::ffi::c_int,
    pub next: *mut memblock_s,
    pub prev: *mut memblock_s,
}
pub type byte = uint8_t;
pub type uint8_t = __uint8_t;
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const MEM_ALIGN: usize = ::core::mem::size_of::<*mut ::core::ffi::c_void>();
pub const ZONEID: ::core::ffi::c_int = 0x1d4a11 as ::core::ffi::c_int;
#[no_mangle]
pub static mut mainzone: *mut memzone_t = ::core::ptr::null_mut::<memzone_t>();
#[no_mangle]
pub unsafe extern "C" fn Z_ClearZone(mut zone: *mut memzone_t) {
    let mut block: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    block = (zone as *mut byte).offset(::core::mem::size_of::<memzone_t>() as usize as isize)
        as *mut memblock_t;
    (*zone).blocklist.prev = block as *mut memblock_s;
    (*zone).blocklist.next = (*zone).blocklist.prev;
    (*zone).blocklist.user = zone as *mut ::core::ffi::c_void as *mut *mut ::core::ffi::c_void;
    (*zone).blocklist.tag = PU_STATIC as ::core::ffi::c_int;
    (*zone).rover = block;
    (*block).next = &raw mut (*zone).blocklist as *mut memblock_s;
    (*block).prev = (*block).next;
    (*block).tag = PU_FREE as ::core::ffi::c_int;
    (*block).size = ((*zone).size as usize)
        .wrapping_sub(::core::mem::size_of::<memzone_t>() as usize)
        as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn Z_Init() {
    let mut block: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    let mut size: ::core::ffi::c_int = 0;
    mainzone = I_ZoneBase(&raw mut size) as *mut memzone_t;
    (*mainzone).size = size;
    block = (mainzone as *mut byte).offset(::core::mem::size_of::<memzone_t>() as usize as isize)
        as *mut memblock_t;
    (*mainzone).blocklist.prev = block as *mut memblock_s;
    (*mainzone).blocklist.next = (*mainzone).blocklist.prev;
    (*mainzone).blocklist.user =
        mainzone as *mut ::core::ffi::c_void as *mut *mut ::core::ffi::c_void;
    (*mainzone).blocklist.tag = PU_STATIC as ::core::ffi::c_int;
    (*mainzone).rover = block;
    (*block).next = &raw mut (*mainzone).blocklist as *mut memblock_s;
    (*block).prev = (*block).next;
    (*block).tag = PU_FREE as ::core::ffi::c_int;
    (*block).size = ((*mainzone).size as usize)
        .wrapping_sub(::core::mem::size_of::<memzone_t>() as usize)
        as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn Z_Free(mut ptr: *mut ::core::ffi::c_void) {
    let mut block: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    let mut other: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    block = (ptr as *mut byte).offset(-(::core::mem::size_of::<memblock_t>() as usize as isize))
        as *mut memblock_t;
    if (*block).id != ZONEID {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Z_Free: freed a pointer without ZONEID\0".as_ptr() as *const ::core::ffi::c_char,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    if (*block).tag != PU_FREE as ::core::ffi::c_int && !(*block).user.is_null() {
        *(*block).user = ::core::ptr::null_mut::<::core::ffi::c_void>();
    }
    (*block).tag = PU_FREE as ::core::ffi::c_int;
    (*block).user = ::core::ptr::null_mut::<*mut ::core::ffi::c_void>();
    (*block).id = 0 as ::core::ffi::c_int;
    other = (*block).prev as *mut memblock_t;
    if (*other).tag == PU_FREE as ::core::ffi::c_int {
        (*other).size += (*block).size;
        (*other).next = (*block).next;
        (*(*other).next).prev = other as *mut memblock_s;
        if block == (*mainzone).rover {
            (*mainzone).rover = other;
        }
        block = other;
    }
    other = (*block).next as *mut memblock_t;
    if (*other).tag == PU_FREE as ::core::ffi::c_int {
        (*block).size += (*other).size;
        (*block).next = (*other).next;
        (*(*block).next).prev = block as *mut memblock_s;
        if other == (*mainzone).rover {
            (*mainzone).rover = block;
        }
    }
}
pub const MINFRAGMENT: ::core::ffi::c_int = 64 as ::core::ffi::c_int;
#[no_mangle]
pub unsafe extern "C" fn Z_Malloc(
    mut size: ::core::ffi::c_int,
    mut tag: ::core::ffi::c_int,
    mut user: *mut ::core::ffi::c_void,
) -> *mut ::core::ffi::c_void {
    let mut extra: ::core::ffi::c_int = 0;
    let mut start: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    let mut rover: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    let mut newblock: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    let mut base: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    let mut result: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
    size = ((size as usize)
        .wrapping_add(MEM_ALIGN)
        .wrapping_sub(1 as usize)
        & !MEM_ALIGN.wrapping_sub(1 as usize)) as ::core::ffi::c_int;
    size = (size as ::core::ffi::c_ulong)
        .wrapping_add(::core::mem::size_of::<memblock_t>() as usize as ::core::ffi::c_ulong)
        as ::core::ffi::c_int;
    base = (*mainzone).rover;
    if (*(*base).prev).tag == PU_FREE as ::core::ffi::c_int {
        base = (*base).prev as *mut memblock_t;
    }
    rover = base;
    start = (*base).prev as *mut memblock_t;
    loop {
        if rover == start {
            let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
            snprintf(
                &raw mut _i_err_buf as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                b"Z_Malloc: failed on allocation of %i bytes\0".as_ptr()
                    as *const ::core::ffi::c_char,
                size,
            );
            I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
        }
        if (*rover).tag != PU_FREE as ::core::ffi::c_int {
            if (*rover).tag < PU_PURGELEVEL as ::core::ffi::c_int {
                rover = (*rover).next as *mut memblock_t;
                base = rover;
            } else {
                base = (*base).prev as *mut memblock_t;
                Z_Free(
                    (rover as *mut byte)
                        .offset(::core::mem::size_of::<memblock_t>() as usize as isize)
                        as *mut ::core::ffi::c_void,
                );
                base = (*base).next as *mut memblock_t;
                rover = (*base).next as *mut memblock_t;
            }
        } else {
            rover = (*rover).next as *mut memblock_t;
        }
        if !((*base).tag != PU_FREE as ::core::ffi::c_int || (*base).size < size) {
            break;
        }
    }
    extra = (*base).size - size;
    if extra > MINFRAGMENT {
        newblock = (base as *mut byte).offset(size as isize) as *mut memblock_t;
        (*newblock).size = extra;
        (*newblock).tag = PU_FREE as ::core::ffi::c_int;
        (*newblock).user = ::core::ptr::null_mut::<*mut ::core::ffi::c_void>();
        (*newblock).prev = base as *mut memblock_s;
        (*newblock).next = (*base).next;
        (*(*newblock).next).prev = newblock as *mut memblock_s;
        (*base).next = newblock as *mut memblock_s;
        (*base).size = size;
    }
    if user.is_null() && tag >= PU_PURGELEVEL as ::core::ffi::c_int {
        let mut _i_err_buf_0: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf_0 as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Z_Malloc: an owner is required for purgable blocks\0".as_ptr()
                as *const ::core::ffi::c_char,
        );
        I_ErrorV(&raw mut _i_err_buf_0 as *mut ::core::ffi::c_char);
    }
    (*base).user = user as *mut *mut ::core::ffi::c_void;
    (*base).tag = tag;
    result = (base as *mut byte).offset(::core::mem::size_of::<memblock_t>() as usize as isize)
        as *mut ::core::ffi::c_void;
    if !(*base).user.is_null() {
        *(*base).user = result;
    }
    (*mainzone).rover = (*base).next as *mut memblock_t;
    (*base).id = ZONEID;
    return result;
}
#[no_mangle]
pub unsafe extern "C" fn Z_FreeTags(
    mut lowtag: ::core::ffi::c_int,
    mut hightag: ::core::ffi::c_int,
) {
    let mut block: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    let mut next: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    block = (*mainzone).blocklist.next as *mut memblock_t;
    while block != &raw mut (*mainzone).blocklist {
        next = (*block).next as *mut memblock_t;
        if !((*block).tag == PU_FREE as ::core::ffi::c_int) {
            if (*block).tag >= lowtag && (*block).tag <= hightag {
                Z_Free(
                    (block as *mut byte)
                        .offset(::core::mem::size_of::<memblock_t>() as usize as isize)
                        as *mut ::core::ffi::c_void,
                );
            }
        }
        block = next;
    }
}
#[no_mangle]
pub unsafe extern "C" fn Z_DumpHeap(
    mut lowtag: ::core::ffi::c_int,
    mut hightag: ::core::ffi::c_int,
) {
    let mut block: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    printf(
        b"zone size: %i  location: %p\n\0".as_ptr() as *const ::core::ffi::c_char,
        (*mainzone).size,
        mainzone,
    );
    printf(
        b"tag range: %i to %i\n\0".as_ptr() as *const ::core::ffi::c_char,
        lowtag,
        hightag,
    );
    block = (*mainzone).blocklist.next as *mut memblock_t;
    loop {
        if (*block).tag >= lowtag && (*block).tag <= hightag {
            printf(
                b"block:%p    size:%7i    user:%p    tag:%3i\n\0".as_ptr()
                    as *const ::core::ffi::c_char,
                block,
                (*block).size,
                (*block).user,
                (*block).tag,
            );
        }
        if (*block).next == &raw mut (*mainzone).blocklist {
            break;
        }
        if (block as *mut byte).offset((*block).size as isize) != (*block).next as *mut byte {
            printf(
                b"ERROR: block size does not touch the next block\n\0".as_ptr()
                    as *const ::core::ffi::c_char,
            );
        }
        if (*(*block).next).prev != block {
            printf(
                b"ERROR: next block doesn't have proper back link\n\0".as_ptr()
                    as *const ::core::ffi::c_char,
            );
        }
        if (*block).tag == PU_FREE as ::core::ffi::c_int
            && (*(*block).next).tag == PU_FREE as ::core::ffi::c_int
        {
            printf(b"ERROR: two consecutive free blocks\n\0".as_ptr() as *const ::core::ffi::c_char);
        }
        block = (*block).next as *mut memblock_t;
    }
}
#[no_mangle]
pub unsafe extern "C" fn Z_FileDumpHeap(mut f: *mut FILE) {
    let mut block: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    fprintf(
        f,
        b"zone size: %i  location: %p\n\0".as_ptr() as *const ::core::ffi::c_char,
        (*mainzone).size,
        mainzone,
    );
    block = (*mainzone).blocklist.next as *mut memblock_t;
    loop {
        fprintf(
            f,
            b"block:%p    size:%7i    user:%p    tag:%3i\n\0".as_ptr()
                as *const ::core::ffi::c_char,
            block,
            (*block).size,
            (*block).user,
            (*block).tag,
        );
        if (*block).next == &raw mut (*mainzone).blocklist {
            break;
        }
        if (block as *mut byte).offset((*block).size as isize) != (*block).next as *mut byte {
            fprintf(
                f,
                b"ERROR: block size does not touch the next block\n\0".as_ptr()
                    as *const ::core::ffi::c_char,
            );
        }
        if (*(*block).next).prev != block {
            fprintf(
                f,
                b"ERROR: next block doesn't have proper back link\n\0".as_ptr()
                    as *const ::core::ffi::c_char,
            );
        }
        if (*block).tag == PU_FREE as ::core::ffi::c_int
            && (*(*block).next).tag == PU_FREE as ::core::ffi::c_int
        {
            fprintf(
                f,
                b"ERROR: two consecutive free blocks\n\0".as_ptr() as *const ::core::ffi::c_char,
            );
        }
        block = (*block).next as *mut memblock_t;
    }
}
#[no_mangle]
pub unsafe extern "C" fn Z_CheckHeap() {
    let mut block: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    block = (*mainzone).blocklist.next as *mut memblock_t;
    while !((*block).next == &raw mut (*mainzone).blocklist) {
        if (block as *mut byte).offset((*block).size as isize) != (*block).next as *mut byte {
            let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
            snprintf(
                &raw mut _i_err_buf as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                b"Z_CheckHeap: block size does not touch the next block\n\0".as_ptr()
                    as *const ::core::ffi::c_char,
            );
            I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
        }
        if (*(*block).next).prev != block {
            let mut _i_err_buf_0: [::core::ffi::c_char; 512] = [0; 512];
            snprintf(
                &raw mut _i_err_buf_0 as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                b"Z_CheckHeap: next block doesn't have proper back link\n\0".as_ptr()
                    as *const ::core::ffi::c_char,
            );
            I_ErrorV(&raw mut _i_err_buf_0 as *mut ::core::ffi::c_char);
        }
        if (*block).tag == PU_FREE as ::core::ffi::c_int
            && (*(*block).next).tag == PU_FREE as ::core::ffi::c_int
        {
            let mut _i_err_buf_1: [::core::ffi::c_char; 512] = [0; 512];
            snprintf(
                &raw mut _i_err_buf_1 as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                b"Z_CheckHeap: two consecutive free blocks\n\0".as_ptr()
                    as *const ::core::ffi::c_char,
            );
            I_ErrorV(&raw mut _i_err_buf_1 as *mut ::core::ffi::c_char);
        }
        block = (*block).next as *mut memblock_t;
    }
}
#[no_mangle]
pub unsafe extern "C" fn Z_ChangeTag2(
    mut ptr: *mut ::core::ffi::c_void,
    mut tag: ::core::ffi::c_int,
    mut file: *mut ::core::ffi::c_char,
    mut line: ::core::ffi::c_int,
) {
    let mut block: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    block = (ptr as *mut byte).offset(-(::core::mem::size_of::<memblock_t>() as usize as isize))
        as *mut memblock_t;
    if (*block).id != ZONEID {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"%s:%i: Z_ChangeTag: block without a ZONEID!\0".as_ptr() as *const ::core::ffi::c_char,
            file,
            line,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    if tag >= PU_PURGELEVEL as ::core::ffi::c_int && (*block).user.is_null() {
        let mut _i_err_buf_0: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf_0 as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"%s:%i: Z_ChangeTag: an owner is required for purgable blocks\0".as_ptr()
                as *const ::core::ffi::c_char,
            file,
            line,
        );
        I_ErrorV(&raw mut _i_err_buf_0 as *mut ::core::ffi::c_char);
    }
    (*block).tag = tag;
}
#[no_mangle]
pub unsafe extern "C" fn Z_ChangeUser(
    mut ptr: *mut ::core::ffi::c_void,
    mut user: *mut *mut ::core::ffi::c_void,
) {
    let mut block: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    block = (ptr as *mut byte).offset(-(::core::mem::size_of::<memblock_t>() as usize as isize))
        as *mut memblock_t;
    if (*block).id != ZONEID {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Z_ChangeUser: Tried to change user for invalid block!\0".as_ptr()
                as *const ::core::ffi::c_char,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    (*block).user = user;
    *user = ptr;
}
#[no_mangle]
pub unsafe extern "C" fn Z_FreeMemory() -> ::core::ffi::c_int {
    let mut block: *mut memblock_t = ::core::ptr::null_mut::<memblock_t>();
    let mut free: ::core::ffi::c_int = 0;
    free = 0 as ::core::ffi::c_int;
    block = (*mainzone).blocklist.next as *mut memblock_t;
    while block != &raw mut (*mainzone).blocklist {
        if (*block).tag == PU_FREE as ::core::ffi::c_int
            || (*block).tag >= PU_PURGELEVEL as ::core::ffi::c_int
        {
            free += (*block).size;
        }
        block = (*block).next as *mut memblock_t;
    }
    return free;
}
#[no_mangle]
pub unsafe extern "C" fn Z_ZoneSize() -> ::core::ffi::c_uint {
    return (*mainzone).size as ::core::ffi::c_uint;
}
