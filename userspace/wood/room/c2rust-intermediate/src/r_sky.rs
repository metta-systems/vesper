pub const FRACBITS: ::core::ffi::c_int = 16 as ::core::ffi::c_int;
pub const FRACUNIT: ::core::ffi::c_int = (1 as ::core::ffi::c_int) << FRACBITS;
#[no_mangle]
pub static mut skyflatnum: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut skytexture: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut skytexturemid: ::core::ffi::c_int = 0;
#[no_mangle]
pub unsafe extern "C" fn R_InitSkyMap() {
    skytexturemid = 100 as ::core::ffi::c_int * FRACUNIT;
}
