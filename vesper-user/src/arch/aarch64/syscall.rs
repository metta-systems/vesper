pub fn syscall(number: u64) {
    asm!("svc #1234")
}
