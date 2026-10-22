extern "C" {
    fn strrchr(
        __s: *const ::core::ffi::c_char,
        __c: ::core::ffi::c_int,
    ) -> *mut ::core::ffi::c_char;
    fn strcasecmp(
        __s1: *const ::core::ffi::c_char,
        __s2: *const ::core::ffi::c_char,
    ) -> ::core::ffi::c_int;
}
pub type boolean = ::core::ffi::c_uint;
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const DIR_SEPARATOR: ::core::ffi::c_int = '/' as ::core::ffi::c_int;
#[no_mangle]
pub static mut myargc: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut myargv: *mut *mut ::core::ffi::c_char =
    ::core::ptr::null_mut::<*mut ::core::ffi::c_char>();
#[no_mangle]
pub unsafe extern "C" fn M_CheckParmWithArgs(
    mut check: *mut ::core::ffi::c_char,
    mut num_args: ::core::ffi::c_int,
) -> ::core::ffi::c_int {
    let mut i: ::core::ffi::c_int = 0;
    i = 1 as ::core::ffi::c_int;
    while i < myargc - num_args {
        if strcasecmp(check, *myargv.offset(i as isize)) == 0 {
            return i;
        }
        i += 1;
    }
    return 0 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn M_ParmExists(mut check: *mut ::core::ffi::c_char) -> boolean {
    return (M_CheckParm(check) != 0 as ::core::ffi::c_int) as ::core::ffi::c_int as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn M_CheckParm(mut check: *mut ::core::ffi::c_char) -> ::core::ffi::c_int {
    return M_CheckParmWithArgs(check, 0 as ::core::ffi::c_int);
}
unsafe extern "C" fn LoadResponseFile(mut argv_index: ::core::ffi::c_int) {}
#[no_mangle]
pub unsafe extern "C" fn M_FindResponseFile() {
    let mut i: ::core::ffi::c_int = 0;
    i = 1 as ::core::ffi::c_int;
    while i < myargc {
        if *(*myargv.offset(i as isize)).offset(0 as ::core::ffi::c_int as isize)
            as ::core::ffi::c_int
            == '@' as ::core::ffi::c_int
        {
            LoadResponseFile(i);
        }
        i += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn M_GetExecutableName() -> *mut ::core::ffi::c_char {
    let mut sep: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    sep = strrchr(
        *myargv.offset(0 as ::core::ffi::c_int as isize),
        DIR_SEPARATOR,
    );
    if sep.is_null() {
        return *myargv.offset(0 as ::core::ffi::c_int as isize);
    } else {
        return sep.offset(1 as ::core::ffi::c_int as isize);
    };
}
