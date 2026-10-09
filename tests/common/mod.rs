// Shared integration tests setup
use core::panic::PanicInfo;

#[panic_handler]
fn panicked(info: &PanicInfo) -> ! {
    libtest::panic::handler_for_tests(info)
}

libboot::entry!(test_run_helper);

pub fn test_run_helper(_dtb: u32) -> ! {
    let current_el: u64;
    // SAFETY: Reading CurrentEL has no side effects and is valid during boot.
    unsafe {
        core::arch::asm!(
            "mrs {level}, CurrentEL",
            level = out(reg) current_el,
            options(nomem, nostack, preserves_flags),
        );
    }
    assert_eq!(current_el, 0x8, "the test boot helper must enter from EL2");

    // SAFETY: libboot initialized EL1 control/timer registers and a valid stack.
    // Tests never return to this EL2 frame. Keep its stack, mask DAIF, and enter
    // EL1h (SPSR_EL2 = 0x3c5) before running code that requires kernel privilege.
    unsafe {
        core::arch::asm!(
            "mov x9, sp",
            "msr SP_EL1, x9",
            "mov x9, #0x3c5",
            "msr SPSR_EL2, x9",
            "msr ELR_EL2, x0",
            "eret",
            in("x0") run_tests as *const (),
            options(noreturn),
        );
    }
}

extern "C" fn run_tests() -> ! {
    libconsole::init_logger().unwrap();
    liblog::set_max_level(liblog::Level::Trace); // Allow everything in tests
    crate::test_main();
    libqemu::semihosting::exit_success()
}
