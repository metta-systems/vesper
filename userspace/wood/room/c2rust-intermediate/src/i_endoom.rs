pub type __uint8_t = u8;
pub type uint8_t = __uint8_t;
pub type byte = uint8_t;
#[no_mangle]
pub unsafe extern "C" fn I_Endoom(mut endoom_data: *mut byte) {}
