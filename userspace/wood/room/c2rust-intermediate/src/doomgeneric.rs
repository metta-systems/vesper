extern "C" {
    static mut myargc: ::core::ffi::c_int;
    static mut myargv: *mut *mut ::core::ffi::c_char;
    fn M_FindResponseFile();
    fn malloc(__size: size_t) -> *mut ::core::ffi::c_void;
    fn DG_Init();
    fn D_DoomMain();
}
pub type size_t = usize;
pub type __uint32_t = u32;
pub type uint32_t = __uint32_t;
pub type pixel_t = uint32_t;
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const DOOMGENERIC_RESX: ::core::ffi::c_int = 640 as ::core::ffi::c_int;
pub const DOOMGENERIC_RESY: ::core::ffi::c_int = 400 as ::core::ffi::c_int;
#[no_mangle]
pub static mut DG_ScreenBuffer: *mut pixel_t = ::core::ptr::null_mut::<pixel_t>();
#[no_mangle]
pub unsafe extern "C" fn doomgeneric_Create(
    mut argc: ::core::ffi::c_int,
    mut argv: *mut *mut ::core::ffi::c_char,
) {
    myargc = argc;
    myargv = argv;
    M_FindResponseFile();
    DG_ScreenBuffer =
        malloc((DOOMGENERIC_RESX * DOOMGENERIC_RESY * 4 as ::core::ffi::c_int) as size_t)
            as *mut pixel_t;
    DG_Init();
    D_DoomMain();
}
