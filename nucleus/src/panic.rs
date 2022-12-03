pub fn handler(info: &core::panic::PanicInfo) -> ! {
    // @todo This may fail to print if the panic message is too long for local print buffer.
    crate::println!("{}", info);
    crate::endless_sleep()
}

pub fn handler_for_tests(info: &core::panic::PanicInfo) -> ! {
    crate::println!("\n[failed]\n");
    // @todo This may fail to print if the panic message is too long for local print buffer.
    crate::println!("\nError: {}\n", info);
    crate::qemu::semihosting::exit_failure()
}
