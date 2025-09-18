pub fn handler_for_tests(info: &core::panic::PanicInfo) -> ! {
    liblog::println!("\n[failed]\n");
    // Protect against panic infinite loops if any of the following code panics itself.
    panic_prevent_reenter();
    print_panic_info(info);
    libqemu::semihosting::exit_failure()
}

fn print_panic_info(info: &core::panic::PanicInfo) {
    let (location, line, column) = match info.location() {
        Some(loc) => (loc.file(), loc.line(), loc.column()),
        _ => ("???", 0, 0),
    };

    // @todo This may fail to print if the panic message is too long for local print buffer.
    liblog::println!(
        "Kernel panic!\n\n\
        Panic location:\n      File '{}', line {}, column {}\n\n\
        {}",
        location,
        line,
        column,
        info.message(),
    );
}

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

    libqemu::semihosting::exit_failure();
}
