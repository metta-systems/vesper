#[cfg(not(test))]
#[panic_handler]
fn panicked(info: &core::panic::PanicInfo) -> ! {
    // @todo This may fail to print if the panic message is too long for local print buffer.
    crate::println!("{}", info);
    crate::endless_sleep()
}

#[cfg(test)]
#[panic_handler]
fn panicked(info: &core::panic::PanicInfo) -> ! {
    crate::println!("\n[failed]\n");
    // @todo This may fail to print if the panic message is too long for local print buffer.
    crate::println!("\nError: {}\n", info);
    crate::qemu::semihosting::exit_failure()
}
