extern "C" {
    fn M_CheckParm(check: *mut ::core::ffi::c_char) -> ::core::ffi::c_int;
    static mut stdc_wad_file: wad_file_class_t;
}
pub type size_t = usize;
pub type __uint8_t = u8;
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
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
static mut wad_file_classes: [*mut wad_file_class_t; 1] =
    unsafe { [&raw const stdc_wad_file as *mut wad_file_class_t] };
#[no_mangle]
pub unsafe extern "C" fn W_OpenFile(mut path: *mut ::core::ffi::c_char) -> *mut wad_file_t {
    let mut result: *mut wad_file_t = ::core::ptr::null_mut::<wad_file_t>();
    let mut i: ::core::ffi::c_int = 0;
    if M_CheckParm(b"-mmap\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char)
        == 0
    {
        return stdc_wad_file.OpenFile.expect("non-null function pointer")(path);
    }
    result = ::core::ptr::null_mut::<wad_file_t>();
    i = 0 as ::core::ffi::c_int;
    while (i as usize)
        < (::core::mem::size_of::<[*mut wad_file_class_t; 1]>() as usize)
            .wrapping_div(::core::mem::size_of::<*mut wad_file_class_t>() as usize)
    {
        result = (*wad_file_classes[i as usize])
            .OpenFile
            .expect("non-null function pointer")(path);
        if !result.is_null() {
            break;
        }
        i += 1;
    }
    return result;
}
#[no_mangle]
pub unsafe extern "C" fn W_CloseFile(mut wad: *mut wad_file_t) {
    (*(*wad).file_class)
        .CloseFile
        .expect("non-null function pointer")(wad);
}
#[no_mangle]
pub unsafe extern "C" fn W_Read(
    mut wad: *mut wad_file_t,
    mut offset: ::core::ffi::c_uint,
    mut buffer: *mut ::core::ffi::c_void,
    mut buffer_len: size_t,
) -> size_t {
    return (*(*wad).file_class)
        .Read
        .expect("non-null function pointer")(wad, offset, buffer, buffer_len);
}
