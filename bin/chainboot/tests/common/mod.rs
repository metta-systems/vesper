// Shared integration tests setup
core::arch::global_asm!(
    core::include_str!("../../src/boot.s"),
    CONST_BOOT_CORE_ID = const 0,
    CONST_CORE_ID_MASK = const 0b11,
);

use core::panic::PanicInfo;

#[panic_handler]
fn panicked(info: &PanicInfo) -> ! {
    libtest::panic::handler_for_tests(info)
}

// we cannot use libboot::entry! macro here, as we link to a different boot code
#[unsafe(export_name = "kernel_init")]
pub unsafe fn test_run_helper(_max_kernel_size: u64) -> ! {
    libconsole::init_logger().unwrap();
    liblog::set_max_level(liblog::Level::Trace); // Allow everything in tests
    crate::test_main();
    libqemu::semihosting::exit_success()
}
