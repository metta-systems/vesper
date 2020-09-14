#[cfg(test)]
pub fn semihosting_sys_write0_call(text: &str) {
    // SAFETY: text must be \0-terminated!
    unsafe {
        asm!(
            "mov w0, #0x04
             hlt #0xF000"
             , in("x1") text.as_ptr() as u64
        );
    }
}
