extern "C" {
    fn memcpy(
        __dest: *mut ::core::ffi::c_void,
        __src: *const ::core::ffi::c_void,
        __n: size_t,
    ) -> *mut ::core::ffi::c_void;
    fn M_ParmExists(check: *mut ::core::ffi::c_char) -> boolean;
}
pub type size_t = usize;
pub type boolean = ::core::ffi::c_uint;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct wbplayerstruct_t {
    pub in_0: boolean,
    pub skills: ::core::ffi::c_int,
    pub sitems: ::core::ffi::c_int,
    pub ssecret: ::core::ffi::c_int,
    pub stime: ::core::ffi::c_int,
    pub frags: [::core::ffi::c_int; 4],
    pub score: ::core::ffi::c_int,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct wbstartstruct_t {
    pub epsd: ::core::ffi::c_int,
    pub didsecret: boolean,
    pub last: ::core::ffi::c_int,
    pub next: ::core::ffi::c_int,
    pub maxkills: ::core::ffi::c_int,
    pub maxitems: ::core::ffi::c_int,
    pub maxsecret: ::core::ffi::c_int,
    pub maxfrags: ::core::ffi::c_int,
    pub partime: ::core::ffi::c_int,
    pub pnum: ::core::ffi::c_int,
    pub plyr: [wbplayerstruct_t; 4],
}
pub const MAX_CAPTURES: ::core::ffi::c_int = 32 as ::core::ffi::c_int;
static mut captured_stats: [wbstartstruct_t; 32] = [wbstartstruct_t {
    epsd: 0,
    didsecret: 0,
    last: 0,
    next: 0,
    maxkills: 0,
    maxitems: 0,
    maxsecret: 0,
    maxfrags: 0,
    partime: 0,
    pnum: 0,
    plyr: [wbplayerstruct_t {
        in_0: 0,
        skills: 0,
        sitems: 0,
        ssecret: 0,
        stime: 0,
        frags: [0; 4],
        score: 0,
    }; 4],
}; 32];
static mut num_captured_stats: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
#[no_mangle]
pub unsafe extern "C" fn StatCopy(mut stats: *mut wbstartstruct_t) {
    if M_ParmExists(
        b"-statdump\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char
    ) != 0
        && num_captured_stats < MAX_CAPTURES
    {
        memcpy(
            (&raw mut captured_stats as *mut wbstartstruct_t).offset(num_captured_stats as isize)
                as *mut wbstartstruct_t as *mut ::core::ffi::c_void,
            stats as *const ::core::ffi::c_void,
            ::core::mem::size_of::<wbstartstruct_t>() as size_t,
        );
        num_captured_stats += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn StatDump() {}
