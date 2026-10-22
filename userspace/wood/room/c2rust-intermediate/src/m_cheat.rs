extern "C" {
    fn memcpy(
        __dest: *mut ::core::ffi::c_void,
        __src: *const ::core::ffi::c_void,
        __n: size_t,
    ) -> *mut ::core::ffi::c_void;
    fn strlen(__s: *const ::core::ffi::c_char) -> size_t;
}
pub type size_t = usize;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct cheatseq_t {
    pub sequence: [::core::ffi::c_char; 25],
    pub sequence_len: size_t,
    pub parameter_chars: ::core::ffi::c_int,
    pub chars_read: size_t,
    pub param_chars_read: ::core::ffi::c_int,
    pub parameter_buf: [::core::ffi::c_char; 5],
}
pub const true_0: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
#[no_mangle]
pub unsafe extern "C" fn cht_CheckCheat(
    mut cht: *mut cheatseq_t,
    mut key: ::core::ffi::c_char,
) -> ::core::ffi::c_int {
    if (*cht).parameter_chars > 0 as ::core::ffi::c_int
        && strlen(&raw mut (*cht).sequence as *mut ::core::ffi::c_char) < (*cht).sequence_len
    {
        return false_0;
    }
    if (*cht).chars_read < strlen(&raw mut (*cht).sequence as *mut ::core::ffi::c_char) {
        if key as ::core::ffi::c_int
            == (*cht).sequence[(*cht).chars_read as usize] as ::core::ffi::c_int
        {
            (*cht).chars_read = (*cht).chars_read.wrapping_add(1);
        } else {
            (*cht).chars_read = 0 as size_t;
        }
        (*cht).param_chars_read = 0 as ::core::ffi::c_int;
    } else if (*cht).param_chars_read < (*cht).parameter_chars {
        (*cht).parameter_buf[(*cht).param_chars_read as usize] = key;
        (*cht).param_chars_read += 1;
    }
    if (*cht).chars_read >= strlen(&raw mut (*cht).sequence as *mut ::core::ffi::c_char)
        && (*cht).param_chars_read >= (*cht).parameter_chars
    {
        (*cht).param_chars_read = 0 as ::core::ffi::c_int;
        (*cht).chars_read = (*cht).param_chars_read as size_t;
        return true_0;
    }
    return false_0;
}
#[no_mangle]
pub unsafe extern "C" fn cht_GetParam(
    mut cht: *mut cheatseq_t,
    mut buffer: *mut ::core::ffi::c_char,
) {
    memcpy(
        buffer as *mut ::core::ffi::c_void,
        &raw mut (*cht).parameter_buf as *mut ::core::ffi::c_char as *const ::core::ffi::c_void,
        (*cht).parameter_chars as size_t,
    );
}
