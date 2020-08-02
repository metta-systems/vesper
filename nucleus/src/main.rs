#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn _boot_cores() -> ! {
    loop {}
}

#[panic_handler]
fn panicked(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
