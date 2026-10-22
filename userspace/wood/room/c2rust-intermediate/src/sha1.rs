extern "C" {
    fn memcpy(
        __dest: *mut ::core::ffi::c_void,
        __src: *const ::core::ffi::c_void,
        __n: size_t,
    ) -> *mut ::core::ffi::c_void;
    fn memset(
        __s: *mut ::core::ffi::c_void,
        __c: ::core::ffi::c_int,
        __n: size_t,
    ) -> *mut ::core::ffi::c_void;
    fn strlen(__s: *const ::core::ffi::c_char) -> size_t;
}
pub type size_t = usize;
pub type __uint8_t = u8;
pub type __uint32_t = u32;
pub type uint8_t = __uint8_t;
pub type uint32_t = __uint32_t;
pub type byte = uint8_t;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct sha1_context_s {
    pub h0: uint32_t,
    pub h1: uint32_t,
    pub h2: uint32_t,
    pub h3: uint32_t,
    pub h4: uint32_t,
    pub nblocks: uint32_t,
    pub buf: [byte; 64],
    pub count: ::core::ffi::c_int,
}
pub type sha1_context_t = sha1_context_s;
pub type sha1_digest_t = [byte; 20];
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
#[no_mangle]
pub unsafe extern "C" fn SHA1_Init(mut hd: *mut sha1_context_t) {
    (*hd).h0 = 0x67452301 as uint32_t;
    (*hd).h1 = 0xefcdab89 as ::core::ffi::c_uint as uint32_t;
    (*hd).h2 = 0x98badcfe as ::core::ffi::c_uint as uint32_t;
    (*hd).h3 = 0x10325476 as uint32_t;
    (*hd).h4 = 0xc3d2e1f0 as ::core::ffi::c_uint as uint32_t;
    (*hd).nblocks = 0 as uint32_t;
    (*hd).count = 0 as ::core::ffi::c_int;
}
unsafe extern "C" fn Transform(mut hd: *mut sha1_context_t, mut data: *mut byte) {
    let mut a: uint32_t = 0;
    let mut b: uint32_t = 0;
    let mut c: uint32_t = 0;
    let mut d: uint32_t = 0;
    let mut e: uint32_t = 0;
    let mut tm: uint32_t = 0;
    let mut x: [uint32_t; 16] = [0; 16];
    a = (*hd).h0;
    b = (*hd).h1;
    c = (*hd).h2;
    d = (*hd).h3;
    e = (*hd).h4;
    let mut i: ::core::ffi::c_int = 0;
    let mut p2: *mut byte = ::core::ptr::null_mut::<byte>();
    i = 0 as ::core::ffi::c_int;
    p2 = &raw mut x as *mut uint32_t as *mut byte;
    while i < 16 as ::core::ffi::c_int {
        let c2rust_fresh4 = data;
        data = data.offset(1);
        *p2.offset(3 as ::core::ffi::c_int as isize) = *c2rust_fresh4;
        let c2rust_fresh5 = data;
        data = data.offset(1);
        *p2.offset(2 as ::core::ffi::c_int as isize) = *c2rust_fresh5;
        let c2rust_fresh6 = data;
        data = data.offset(1);
        *p2.offset(1 as ::core::ffi::c_int as isize) = *c2rust_fresh6;
        let c2rust_fresh7 = data;
        data = data.offset(1);
        *p2.offset(0 as ::core::ffi::c_int as isize) = *c2rust_fresh7;
        i += 1;
        p2 = p2.offset(4 as ::core::ffi::c_int as isize);
    }
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d ^ b & (c ^ d)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[0 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c ^ a & (b ^ c)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[1 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b ^ e & (a ^ b)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[2 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a ^ d & (e ^ a)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[3 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e ^ c & (d ^ e)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[4 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d ^ b & (c ^ d)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[5 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c ^ a & (b ^ c)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[6 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b ^ e & (a ^ b)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[7 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a ^ d & (e ^ a)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[8 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e ^ c & (d ^ e)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[9 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d ^ b & (c ^ d)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[10 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c ^ a & (b ^ c)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[11 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b ^ e & (a ^ b)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[12 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a ^ d & (e ^ a)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[13 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e ^ c & (d ^ e)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[14 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d ^ b & (c ^ d)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[15 as ::core::ffi::c_int as usize] as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(16 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(16 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(16 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(16 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(16 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c ^ a & (b ^ c)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[(16 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(17 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(17 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(17 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(17 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(17 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b ^ e & (a ^ b)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[(17 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(18 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(18 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(18 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(18 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(18 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a ^ d & (e ^ a)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[(18 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(19 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(19 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(19 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(19 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(19 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e ^ c & (d ^ e)) as ::core::ffi::c_long
            + 0x5a827999 as ::core::ffi::c_long
            + x[(19 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(20 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(20 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(20 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(20 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(20 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b ^ c ^ d) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(20 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(21 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(21 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(21 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(21 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(21 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a ^ b ^ c) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(21 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(22 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(22 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(22 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(22 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(22 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e ^ a ^ b) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(22 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(23 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(23 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(23 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(23 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(23 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d ^ e ^ a) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(23 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(24 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(24 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(24 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(24 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(24 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c ^ d ^ e) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(24 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(25 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(25 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(25 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(25 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(25 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b ^ c ^ d) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(25 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(26 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(26 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(26 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(26 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(26 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a ^ b ^ c) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(26 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(27 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(27 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(27 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(27 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(27 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e ^ a ^ b) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(27 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(28 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(28 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(28 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(28 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(28 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d ^ e ^ a) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(28 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(29 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(29 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(29 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(29 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(29 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c ^ d ^ e) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(29 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(30 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(30 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(30 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(30 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(30 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b ^ c ^ d) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(30 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(31 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(31 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(31 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(31 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(31 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a ^ b ^ c) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(31 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(32 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(32 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(32 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(32 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(32 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e ^ a ^ b) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(32 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(33 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(33 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(33 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(33 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(33 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d ^ e ^ a) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(33 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(34 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(34 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(34 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(34 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(34 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c ^ d ^ e) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(34 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(35 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(35 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(35 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(35 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(35 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b ^ c ^ d) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(35 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(36 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(36 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(36 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(36 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(36 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a ^ b ^ c) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(36 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(37 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(37 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(37 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(37 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(37 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e ^ a ^ b) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(37 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(38 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(38 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(38 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(38 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(38 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d ^ e ^ a) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(38 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(39 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(39 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(39 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(39 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(39 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c ^ d ^ e) as ::core::ffi::c_long
            + 0x6ed9eba1 as ::core::ffi::c_long
            + x[(39 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(40 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(40 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(40 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(40 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(40 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b & c | d & (b | c)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(40 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(41 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(41 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(41 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(41 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(41 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a & b | c & (a | b)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(41 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(42 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(42 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(42 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(42 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(42 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e & a | b & (e | a)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(42 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(43 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(43 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(43 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(43 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(43 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d & e | a & (d | e)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(43 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(44 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(44 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(44 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(44 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(44 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c & d | e & (c | d)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(44 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(45 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(45 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(45 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(45 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(45 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b & c | d & (b | c)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(45 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(46 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(46 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(46 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(46 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(46 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a & b | c & (a | b)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(46 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(47 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(47 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(47 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(47 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(47 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e & a | b & (e | a)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(47 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(48 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(48 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(48 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(48 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(48 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d & e | a & (d | e)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(48 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(49 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(49 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(49 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(49 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(49 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c & d | e & (c | d)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(49 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(50 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(50 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(50 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(50 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(50 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b & c | d & (b | c)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(50 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(51 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(51 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(51 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(51 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(51 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a & b | c & (a | b)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(51 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(52 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(52 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(52 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(52 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(52 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e & a | b & (e | a)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(52 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(53 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(53 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(53 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(53 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(53 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d & e | a & (d | e)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(53 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(54 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(54 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(54 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(54 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(54 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c & d | e & (c | d)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(54 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(55 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(55 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(55 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(55 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(55 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b & c | d & (b | c)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(55 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(56 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(56 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(56 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(56 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(56 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a & b | c & (a | b)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(56 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(57 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(57 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(57 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(57 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(57 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e & a | b & (e | a)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(57 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(58 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(58 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(58 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(58 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(58 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d & e | a & (d | e)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(58 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(59 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(59 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(59 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(59 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(59 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c & d | e & (c | d)) as ::core::ffi::c_long
            + 0x8f1bbcdc as ::core::ffi::c_long
            + x[(59 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(60 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(60 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(60 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(60 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(60 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b ^ c ^ d) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(60 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(61 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(61 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(61 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(61 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(61 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a ^ b ^ c) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(61 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(62 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(62 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(62 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(62 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(62 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e ^ a ^ b) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(62 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(63 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(63 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(63 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(63 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(63 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d ^ e ^ a) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(63 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(64 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(64 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(64 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(64 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(64 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c ^ d ^ e) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(64 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(65 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(65 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(65 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(65 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(65 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b ^ c ^ d) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(65 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(66 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(66 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(66 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(66 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(66 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a ^ b ^ c) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(66 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(67 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(67 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(67 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(67 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(67 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e ^ a ^ b) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(67 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(68 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(68 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(68 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(68 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(68 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d ^ e ^ a) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(68 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(69 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(69 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(69 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(69 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(69 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c ^ d ^ e) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(69 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(70 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(70 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(70 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(70 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(70 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b ^ c ^ d) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(70 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(71 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(71 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(71 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(71 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(71 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a ^ b ^ c) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(71 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(72 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(72 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(72 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(72 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(72 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e ^ a ^ b) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(72 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(73 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(73 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(73 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(73 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(73 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d ^ e ^ a) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(73 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(74 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(74 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(74 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(74 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(74 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c ^ d ^ e) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(74 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(75 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(75 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(75 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(75 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(75 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    e = (e as ::core::ffi::c_long
        + ((a << 5 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(b ^ c ^ d) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(75 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    b = b << 30 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(76 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(76 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(76 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(76 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(76 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    d = (d as ::core::ffi::c_long
        + ((e << 5 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(a ^ b ^ c) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(76 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    a = a << 30 as ::core::ffi::c_int | a >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(77 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(77 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(77 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(77 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(77 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    c = (c as ::core::ffi::c_long
        + ((d << 5 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(e ^ a ^ b) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(77 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    e = e << 30 as ::core::ffi::c_int | e >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(78 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(78 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(78 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(78 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(78 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    b = (b as ::core::ffi::c_long
        + ((c << 5 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(d ^ e ^ a) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(78 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    d = d << 30 as ::core::ffi::c_int | d >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    tm = x[(79 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
        ^ x[(79 as ::core::ffi::c_int - 14 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(79 as ::core::ffi::c_int - 8 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize]
        ^ x[(79 as ::core::ffi::c_int - 3 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int)
            as usize];
    x[(79 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize] =
        tm << 1 as ::core::ffi::c_int | tm >> 32 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
    a = (a as ::core::ffi::c_long
        + ((b << 5 as ::core::ffi::c_int | b >> 32 as ::core::ffi::c_int - 5 as ::core::ffi::c_int)
            .wrapping_add(c ^ d ^ e) as ::core::ffi::c_long
            + 0xca62c1d6 as ::core::ffi::c_long
            + x[(79 as ::core::ffi::c_int & 0xf as ::core::ffi::c_int) as usize]
                as ::core::ffi::c_long)) as uint32_t;
    c = c << 30 as ::core::ffi::c_int | c >> 32 as ::core::ffi::c_int - 30 as ::core::ffi::c_int;
    (*hd).h0 = (*hd).h0.wrapping_add(a);
    (*hd).h1 = (*hd).h1.wrapping_add(b);
    (*hd).h2 = (*hd).h2.wrapping_add(c);
    (*hd).h3 = (*hd).h3.wrapping_add(d);
    (*hd).h4 = (*hd).h4.wrapping_add(e);
}
#[no_mangle]
pub unsafe extern "C" fn SHA1_Update(
    mut hd: *mut sha1_context_t,
    mut inbuf: *mut byte,
    mut inlen: size_t,
) {
    if (*hd).count == 64 as ::core::ffi::c_int {
        Transform(hd, &raw mut (*hd).buf as *mut byte);
        (*hd).count = 0 as ::core::ffi::c_int;
        (*hd).nblocks = (*hd).nblocks.wrapping_add(1);
    }
    if inbuf.is_null() {
        return;
    }
    if (*hd).count != 0 {
        while inlen != 0 && (*hd).count < 64 as ::core::ffi::c_int {
            let c2rust_fresh0 = inbuf;
            inbuf = inbuf.offset(1);
            let c2rust_fresh1 = (*hd).count;
            (*hd).count = (*hd).count + 1;
            (*hd).buf[c2rust_fresh1 as usize] = *c2rust_fresh0;
            inlen = inlen.wrapping_sub(1);
        }
        SHA1_Update(hd, ::core::ptr::null_mut::<byte>(), 0 as size_t);
        if inlen == 0 {
            return;
        }
    }
    while inlen >= 64 as size_t {
        Transform(hd, inbuf);
        (*hd).count = 0 as ::core::ffi::c_int;
        (*hd).nblocks = (*hd).nblocks.wrapping_add(1);
        inlen = inlen.wrapping_sub(64 as size_t);
        inbuf = inbuf.offset(64 as ::core::ffi::c_int as isize);
    }
    while inlen != 0 && (*hd).count < 64 as ::core::ffi::c_int {
        let c2rust_fresh2 = inbuf;
        inbuf = inbuf.offset(1);
        let c2rust_fresh3 = (*hd).count;
        (*hd).count = (*hd).count + 1;
        (*hd).buf[c2rust_fresh3 as usize] = *c2rust_fresh2;
        inlen = inlen.wrapping_sub(1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn SHA1_Final(mut digest: *mut byte, mut hd: *mut sha1_context_t) {
    let mut t: uint32_t = 0;
    let mut msb: uint32_t = 0;
    let mut lsb: uint32_t = 0;
    let mut p: *mut byte = ::core::ptr::null_mut::<byte>();
    SHA1_Update(hd, ::core::ptr::null_mut::<byte>(), 0 as size_t);
    t = (*hd).nblocks;
    lsb = t << 6 as ::core::ffi::c_int;
    msb = t >> 26 as ::core::ffi::c_int;
    t = lsb;
    lsb = lsb.wrapping_add((*hd).count as uint32_t);
    if lsb < t {
        msb = msb.wrapping_add(1);
    }
    t = lsb;
    lsb <<= 3 as ::core::ffi::c_int;
    msb <<= 3 as ::core::ffi::c_int;
    msb |= t >> 29 as ::core::ffi::c_int;
    if (*hd).count < 56 as ::core::ffi::c_int {
        let c2rust_fresh8 = (*hd).count;
        (*hd).count = (*hd).count + 1;
        (*hd).buf[c2rust_fresh8 as usize] = 0x80 as byte;
        while (*hd).count < 56 as ::core::ffi::c_int {
            let c2rust_fresh9 = (*hd).count;
            (*hd).count = (*hd).count + 1;
            (*hd).buf[c2rust_fresh9 as usize] = 0 as byte;
        }
    } else {
        let c2rust_fresh10 = (*hd).count;
        (*hd).count = (*hd).count + 1;
        (*hd).buf[c2rust_fresh10 as usize] = 0x80 as byte;
        while (*hd).count < 64 as ::core::ffi::c_int {
            let c2rust_fresh11 = (*hd).count;
            (*hd).count = (*hd).count + 1;
            (*hd).buf[c2rust_fresh11 as usize] = 0 as byte;
        }
        SHA1_Update(hd, ::core::ptr::null_mut::<byte>(), 0 as size_t);
        memset(
            &raw mut (*hd).buf as *mut byte as *mut ::core::ffi::c_void,
            0 as ::core::ffi::c_int,
            56 as size_t,
        );
    }
    (*hd).buf[56 as ::core::ffi::c_int as usize] = (msb >> 24 as ::core::ffi::c_int) as byte;
    (*hd).buf[57 as ::core::ffi::c_int as usize] = (msb >> 16 as ::core::ffi::c_int) as byte;
    (*hd).buf[58 as ::core::ffi::c_int as usize] = (msb >> 8 as ::core::ffi::c_int) as byte;
    (*hd).buf[59 as ::core::ffi::c_int as usize] = msb as byte;
    (*hd).buf[60 as ::core::ffi::c_int as usize] = (lsb >> 24 as ::core::ffi::c_int) as byte;
    (*hd).buf[61 as ::core::ffi::c_int as usize] = (lsb >> 16 as ::core::ffi::c_int) as byte;
    (*hd).buf[62 as ::core::ffi::c_int as usize] = (lsb >> 8 as ::core::ffi::c_int) as byte;
    (*hd).buf[63 as ::core::ffi::c_int as usize] = lsb as byte;
    Transform(hd, &raw mut (*hd).buf as *mut byte);
    p = &raw mut (*hd).buf as *mut byte;
    let c2rust_fresh12 = p;
    p = p.offset(1);
    *c2rust_fresh12 = ((*hd).h0 >> 24 as ::core::ffi::c_int) as byte;
    let c2rust_fresh13 = p;
    p = p.offset(1);
    *c2rust_fresh13 = ((*hd).h0 >> 16 as ::core::ffi::c_int) as byte;
    let c2rust_fresh14 = p;
    p = p.offset(1);
    *c2rust_fresh14 = ((*hd).h0 >> 8 as ::core::ffi::c_int) as byte;
    let c2rust_fresh15 = p;
    p = p.offset(1);
    *c2rust_fresh15 = (*hd).h0 as byte;
    let c2rust_fresh16 = p;
    p = p.offset(1);
    *c2rust_fresh16 = ((*hd).h1 >> 24 as ::core::ffi::c_int) as byte;
    let c2rust_fresh17 = p;
    p = p.offset(1);
    *c2rust_fresh17 = ((*hd).h1 >> 16 as ::core::ffi::c_int) as byte;
    let c2rust_fresh18 = p;
    p = p.offset(1);
    *c2rust_fresh18 = ((*hd).h1 >> 8 as ::core::ffi::c_int) as byte;
    let c2rust_fresh19 = p;
    p = p.offset(1);
    *c2rust_fresh19 = (*hd).h1 as byte;
    let c2rust_fresh20 = p;
    p = p.offset(1);
    *c2rust_fresh20 = ((*hd).h2 >> 24 as ::core::ffi::c_int) as byte;
    let c2rust_fresh21 = p;
    p = p.offset(1);
    *c2rust_fresh21 = ((*hd).h2 >> 16 as ::core::ffi::c_int) as byte;
    let c2rust_fresh22 = p;
    p = p.offset(1);
    *c2rust_fresh22 = ((*hd).h2 >> 8 as ::core::ffi::c_int) as byte;
    let c2rust_fresh23 = p;
    p = p.offset(1);
    *c2rust_fresh23 = (*hd).h2 as byte;
    let c2rust_fresh24 = p;
    p = p.offset(1);
    *c2rust_fresh24 = ((*hd).h3 >> 24 as ::core::ffi::c_int) as byte;
    let c2rust_fresh25 = p;
    p = p.offset(1);
    *c2rust_fresh25 = ((*hd).h3 >> 16 as ::core::ffi::c_int) as byte;
    let c2rust_fresh26 = p;
    p = p.offset(1);
    *c2rust_fresh26 = ((*hd).h3 >> 8 as ::core::ffi::c_int) as byte;
    let c2rust_fresh27 = p;
    p = p.offset(1);
    *c2rust_fresh27 = (*hd).h3 as byte;
    let c2rust_fresh28 = p;
    p = p.offset(1);
    *c2rust_fresh28 = ((*hd).h4 >> 24 as ::core::ffi::c_int) as byte;
    let c2rust_fresh29 = p;
    p = p.offset(1);
    *c2rust_fresh29 = ((*hd).h4 >> 16 as ::core::ffi::c_int) as byte;
    let c2rust_fresh30 = p;
    p = p.offset(1);
    *c2rust_fresh30 = ((*hd).h4 >> 8 as ::core::ffi::c_int) as byte;
    let c2rust_fresh31 = p;
    p = p.offset(1);
    *c2rust_fresh31 = (*hd).h4 as byte;
    memcpy(
        digest as *mut ::core::ffi::c_void,
        &raw mut (*hd).buf as *mut byte as *const ::core::ffi::c_void,
        ::core::mem::size_of::<sha1_digest_t>() as size_t,
    );
}
#[no_mangle]
pub unsafe extern "C" fn SHA1_UpdateInt32(
    mut context: *mut sha1_context_t,
    mut val: ::core::ffi::c_uint,
) {
    let mut buf: [byte; 4] = [0; 4];
    buf[0 as ::core::ffi::c_int as usize] =
        (val >> 24 as ::core::ffi::c_int & 0xff as ::core::ffi::c_uint) as byte;
    buf[1 as ::core::ffi::c_int as usize] =
        (val >> 16 as ::core::ffi::c_int & 0xff as ::core::ffi::c_uint) as byte;
    buf[2 as ::core::ffi::c_int as usize] =
        (val >> 8 as ::core::ffi::c_int & 0xff as ::core::ffi::c_uint) as byte;
    buf[3 as ::core::ffi::c_int as usize] = (val & 0xff as ::core::ffi::c_uint) as byte;
    SHA1_Update(context, &raw mut buf as *mut byte, 4 as size_t);
}
#[no_mangle]
pub unsafe extern "C" fn SHA1_UpdateString(
    mut context: *mut sha1_context_t,
    mut str: *mut ::core::ffi::c_char,
) {
    SHA1_Update(
        context,
        str as *mut byte,
        strlen(str).wrapping_add(1 as size_t),
    );
}
