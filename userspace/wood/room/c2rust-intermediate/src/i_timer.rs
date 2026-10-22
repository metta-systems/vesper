extern "C" {
    fn DG_SleepMs(ms: uint32_t);
    fn DG_GetTicksMs() -> uint32_t;
}
pub type uint32_t = __uint32_t;
pub type __uint32_t = u32;
pub const TICRATE: ::core::ffi::c_int = 35 as ::core::ffi::c_int;
static mut basetime: uint32_t = 0 as uint32_t;
#[no_mangle]
pub unsafe extern "C" fn I_GetTicks() -> ::core::ffi::c_int {
    return DG_GetTicksMs() as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn I_GetTime() -> ::core::ffi::c_int {
    let mut ticks: uint32_t = 0;
    ticks = I_GetTicks() as uint32_t;
    if basetime == 0 as uint32_t {
        basetime = ticks;
    }
    ticks = ticks.wrapping_sub(basetime);
    return ticks
        .wrapping_mul(TICRATE as uint32_t)
        .wrapping_div(1000 as uint32_t) as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn I_GetTimeMS() -> ::core::ffi::c_int {
    let mut ticks: uint32_t = 0;
    ticks = I_GetTicks() as uint32_t;
    if basetime == 0 as uint32_t {
        basetime = ticks;
    }
    return ticks.wrapping_sub(basetime) as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn I_Sleep(mut ms: ::core::ffi::c_int) {
    DG_SleepMs(ms as uint32_t);
}
#[no_mangle]
pub unsafe extern "C" fn I_WaitVBL(mut count: ::core::ffi::c_int) {}
#[no_mangle]
pub unsafe extern "C" fn I_InitTimer() {}
