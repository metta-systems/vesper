//! A panic handler for hardware and for QEMU.
use core::panic::PanicInfo;

fn print_panic_info(info: &PanicInfo) {
    let (location, line, column) = match info.location() {
        Some(loc) => (loc.file(), loc.line(), loc.column()),
        _ => ("???", 0, 0),
    };

    // @todo This may fail to print if the panic message is too long for local print buffer.
    crate::info!(
        "Kernel panic!\n\n\
        Panic location:\n      File '{}', line {}, column {}\n\n\
        {}",
        location,
        line,
        column,
        info.message().unwrap_or(&format_args!("")),
    );
}

pub fn handler(info: &PanicInfo) -> ! {
    // Protect against panic infinite loops if any of the following code panics itself.
    panic_prevent_reenter();
    print_panic_info(info);
    crate::endless_sleep()
}

/// We have two separate handlers because other crates may use machine crate as a dependency for
/// running their tests, and this means machine could be compiled with different features.
pub fn handler_for_tests(info: &PanicInfo) -> ! {
    crate::println!("\n[failed]\n");
    // Protect against panic infinite loops if any of the following code panics itself.
    panic_prevent_reenter();
    print_panic_info(info);
    crate::qemu::semihosting::exit_failure()
}

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

/// Stop immediately if called a second time.
///
/// # Note
///
/// Using atomics here relieves us from needing to use `unsafe` for the static variable.
///
/// On `AArch64`, which is the only implemented architecture at the time of writing this,
/// [`AtomicBool::load`] and [`AtomicBool::store`] are lowered to ordinary load and store
/// instructions. They are therefore safe to use even with MMU + caching deactivated.
///
/// [`AtomicBool::load`]: core::sync::atomic::AtomicBool::load
/// [`AtomicBool::store`]: core::sync::atomic::AtomicBool::store
fn panic_prevent_reenter() {
    use core::sync::atomic::{AtomicBool, Ordering};

    #[cfg(not(target_arch = "aarch64"))]
    compile_error!("Add the target_arch to above check if the following code is safe to use");

    static PANIC_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

    if !PANIC_IN_PROGRESS.load(Ordering::Relaxed) {
        PANIC_IN_PROGRESS.store(true, Ordering::Relaxed);

        return;
    }

    #[cfg(qemu)]
    crate::qemu::semihosting::exit_failure();
    #[cfg(not(qemu))]
    crate::endless_sleep()
}
