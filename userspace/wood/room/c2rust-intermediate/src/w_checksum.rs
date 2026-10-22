extern "C" {
    fn realloc(__ptr: *mut ::core::ffi::c_void, __size: size_t) -> *mut ::core::ffi::c_void;
    fn M_StringCopy(
        dest: *mut ::core::ffi::c_char,
        src: *const ::core::ffi::c_char,
        dest_size: size_t,
    ) -> boolean;
    fn SHA1_Init(context: *mut sha1_context_t);
    fn SHA1_Final(digest: *mut byte, context: *mut sha1_context_t);
    fn SHA1_UpdateInt32(context: *mut sha1_context_t, val: ::core::ffi::c_uint);
    fn SHA1_UpdateString(context: *mut sha1_context_t, str: *mut ::core::ffi::c_char);
    static mut lumpinfo: *mut lumpinfo_t;
    static mut numlumps: ::core::ffi::c_uint;
}
pub type size_t = usize;
pub type __uint8_t = u8;
pub type __uint32_t = u32;
pub type uint8_t = __uint8_t;
pub type uint32_t = __uint32_t;
pub type boolean = ::core::ffi::c_uint;
pub type byte = uint8_t;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct sha1_context_s {
    pub h0: uint32_t,
    pub h1: uint32_t,
    pub h2: uint32_t,
    pub h3: uint32_t,
    pub h4: uint32_t,
    pub nblocks: uint32_t,
    pub buf: [byte; 64],
    pub count: ::core::ffi::c_int,
}
pub type sha1_context_t = sha1_context_s;
pub type lumpinfo_t = lumpinfo_s;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct lumpinfo_s {
    pub name: [::core::ffi::c_char; 8],
    pub wad_file: *mut wad_file_t,
    pub position: ::core::ffi::c_int,
    pub size: ::core::ffi::c_int,
    pub cache: *mut ::core::ffi::c_void,
    pub next: *mut lumpinfo_t,
}
pub type wad_file_t = _wad_file_s;
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
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
static mut open_wadfiles: *mut *mut wad_file_t = ::core::ptr::null_mut::<*mut wad_file_t>();
static mut num_open_wadfiles: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
unsafe extern "C" fn GetFileNumber(mut handle: *mut wad_file_t) -> ::core::ffi::c_int {
    let mut i: ::core::ffi::c_int = 0;
    let mut result: ::core::ffi::c_int = 0;
    i = 0 as ::core::ffi::c_int;
    while i < num_open_wadfiles {
        if *open_wadfiles.offset(i as isize) == handle {
            return i;
        }
        i += 1;
    }
    open_wadfiles = realloc(
        open_wadfiles as *mut ::core::ffi::c_void,
        (::core::mem::size_of::<*mut wad_file_t>() as size_t)
            .wrapping_mul((num_open_wadfiles + 1 as ::core::ffi::c_int) as size_t),
    ) as *mut *mut wad_file_t;
    let ref mut c2rust_fresh0 = *open_wadfiles.offset(num_open_wadfiles as isize);
    *c2rust_fresh0 = handle;
    result = num_open_wadfiles;
    num_open_wadfiles += 1;
    return result;
}
unsafe extern "C" fn ChecksumAddLump(
    mut sha1_context: *mut sha1_context_t,
    mut lump: *mut lumpinfo_t,
) {
    let mut buf: [::core::ffi::c_char; 9] = [0; 9];
    M_StringCopy(
        &raw mut buf as *mut ::core::ffi::c_char,
        &raw mut (*lump).name as *mut ::core::ffi::c_char,
        ::core::mem::size_of::<[::core::ffi::c_char; 9]>() as size_t,
    );
    SHA1_UpdateString(sha1_context, &raw mut buf as *mut ::core::ffi::c_char);
    SHA1_UpdateInt32(
        sha1_context,
        GetFileNumber((*lump).wad_file) as ::core::ffi::c_uint,
    );
    SHA1_UpdateInt32(sha1_context, (*lump).position as ::core::ffi::c_uint);
    SHA1_UpdateInt32(sha1_context, (*lump).size as ::core::ffi::c_uint);
}
#[no_mangle]
pub unsafe extern "C" fn W_Checksum(mut digest: *mut byte) {
    let mut sha1_context: sha1_context_t = sha1_context_t {
        h0: 0,
        h1: 0,
        h2: 0,
        h3: 0,
        h4: 0,
        nblocks: 0,
        buf: [0; 64],
        count: 0,
    };
    let mut i: ::core::ffi::c_uint = 0;
    SHA1_Init(&raw mut sha1_context);
    num_open_wadfiles = 0 as ::core::ffi::c_int;
    i = 0 as ::core::ffi::c_uint;
    while i < numlumps {
        ChecksumAddLump(
            &raw mut sha1_context,
            lumpinfo.offset(i as isize) as *mut lumpinfo_t,
        );
        i = i.wrapping_add(1);
    }
    SHA1_Final(digest, &raw mut sha1_context);
}
