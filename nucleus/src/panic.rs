#[cfg(not(test))]
#[panic_handler]
fn panicked(info: &core::panic::PanicInfo) -> ! {
    crate::println!("{}", info);
    crate::endless_sleep()
}

#[cfg(test)]
#[panic_handler]
fn panicked(info: &core::panic::PanicInfo) -> ! {
    crate::println!("[failed]\nError: {}\n", info);
    crate::qemu::semihosting::exit_failure()
}
