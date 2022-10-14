#[inline(always)]
#[no_mangle]
#[allow(unused)]
#[link_section = ".text.stdmem"]
#[cfg(not(feature = "asm"))]
pub unsafe extern "C" fn local_memcpy(mut dest: *mut u8, mut src: *const u8, n: usize) {
    let dest_end = dest.add(n);
    while dest < dest_end {
        *dest = *src;
        dest = dest.add(1);
        src = src.add(1);
    }
}

#[inline(always)]
#[no_mangle]
#[allow(unused)]
#[link_section = ".text.stdmem"]
#[cfg(not(feature = "asm"))]
pub unsafe extern "C" fn local_memset(mut s: *mut u8, c: u8, n: usize) {
    let end = s.add(n);
    while s < end {
        *s = c;
        s = s.add(1);
    }
}
