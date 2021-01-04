pub fn syscall(_number: u64) {
    unsafe { asm!("svc #1234") }
}
