#[no_mangle]
pub static mut cd_Error: ::core::ffi::c_int = 0;
#[no_mangle]
pub unsafe extern "C" fn I_CDMusInit() -> ::core::ffi::c_int {
    return 0 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn I_CDMusPrintStartup() {}
#[no_mangle]
pub unsafe extern "C" fn I_CDMusPlay(mut track: ::core::ffi::c_int) -> ::core::ffi::c_int {
    return 0 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn I_CDMusStop() -> ::core::ffi::c_int {
    return 0 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn I_CDMusResume() -> ::core::ffi::c_int {
    return 0 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn I_CDMusSetVolume(mut volume: ::core::ffi::c_int) -> ::core::ffi::c_int {
    cd_Error = 0 as ::core::ffi::c_int;
    return 0 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn I_CDMusFirstTrack() -> ::core::ffi::c_int {
    return 0 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn I_CDMusLastTrack() -> ::core::ffi::c_int {
    return 0 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn I_CDMusTrackLength(
    mut track_num: ::core::ffi::c_int,
) -> ::core::ffi::c_int {
    return 0 as ::core::ffi::c_int;
}
