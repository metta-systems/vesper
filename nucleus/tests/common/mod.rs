// Shared integration tests setup
use core::panic::PanicInfo;

#[panic_handler]
fn panicked(info: &PanicInfo) -> ! {
    libtest::panic::handler_for_tests(info)
}

libboot::entry!(test_run_helper);

pub fn test_run_helper() -> ! {
    libconsole::init_logger().unwrap();
    liblog::set_max_level(liblog::Level::Trace); // Allow everything in tests
    crate::test_main();
    libqemu::semihosting::exit_success()
}
