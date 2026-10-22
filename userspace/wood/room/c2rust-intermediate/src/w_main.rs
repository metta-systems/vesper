extern "C" {
    fn D_TryFindWADByName(filename: *mut ::core::ffi::c_char) -> *mut ::core::ffi::c_char;
    static mut myargc: ::core::ffi::c_int;
    static mut myargv: *mut *mut ::core::ffi::c_char;
    fn M_CheckParmWithArgs(
        check: *mut ::core::ffi::c_char,
        num_args: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int;
    fn printf(__format: *const ::core::ffi::c_char, ...) -> ::core::ffi::c_int;
    fn W_AddFile(filename: *mut ::core::ffi::c_char) -> *mut wad_file_t;
}
pub type size_t = usize;
pub type __uint8_t = u8;
pub type uint8_t = __uint8_t;
pub type boolean = ::core::ffi::c_uint;
pub type byte = uint8_t;
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
pub const true_0: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
#[no_mangle]
pub unsafe extern "C" fn W_ParseCommandLine() -> boolean {
    let mut modifiedgame: boolean = false_0 as boolean;
    let mut p: ::core::ffi::c_int = 0;
    p = M_CheckParmWithArgs(
        b"-file\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        1 as ::core::ffi::c_int,
    );
    if p != 0 {
        modifiedgame = true_0 as boolean;
        loop {
            p += 1;
            if !(p != myargc
                && *(*myargv.offset(p as isize)).offset(0 as ::core::ffi::c_int as isize)
                    as ::core::ffi::c_int
                    != '-' as ::core::ffi::c_int)
            {
                break;
            }
            let mut filename: *mut ::core::ffi::c_char =
                ::core::ptr::null_mut::<::core::ffi::c_char>();
            filename = D_TryFindWADByName(*myargv.offset(p as isize));
            printf(
                b" adding %s\n\0".as_ptr() as *const ::core::ffi::c_char,
                filename,
            );
            W_AddFile(filename);
        }
    }
    return modifiedgame;
}
