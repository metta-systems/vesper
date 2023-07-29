use {
    crate::{console, drivers, platform::device_driver},
    core::sync::atomic::{AtomicBool, Ordering},
};

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

/// Initialize the driver subsystem.
///
/// # Safety
///
/// See child function calls.
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
pub unsafe fn init() -> Result<(), &'static str> {
    static INIT_DONE: AtomicBool = AtomicBool::new(false);
    if INIT_DONE.load(Ordering::Relaxed) {
        return Err("Init already done");
    }

    driver_gpio()?;
    #[cfg(not(feature = "noserial"))]
    driver_uart()?;

    INIT_DONE.store(true, Ordering::Relaxed);
    Ok(())
}

//--------------------------------------------------------------------------------------------------
// Global instances
//--------------------------------------------------------------------------------------------------

static MINI_UART: device_driver::MiniUart =
    unsafe { device_driver::MiniUart::new(device_driver::UART1_BASE) };
// static PL011_UART: device_driver::PL011Uart = unsafe { device_driver::PL011Uart::default() };
static GPIO: device_driver::GPIO = unsafe { device_driver::GPIO::new(device_driver::GPIO_BASE) };

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

/// This must be called only after successful init of the UART driver.
fn post_init_uart() -> Result<(), &'static str> {
    console::register_console(&MINI_UART);
    Ok(())
}

// This must be called only after successful init of the GPIO driver.
fn post_init_gpio() -> Result<(), &'static str> {
    device_driver::MiniUart::prepare_gpio(&GPIO);
    Ok(())
}

fn driver_uart() -> Result<(), &'static str> {
    let uart_descriptor = drivers::DeviceDriverDescriptor::new(&MINI_UART, Some(post_init_uart));
    drivers::driver_manager().register_driver(uart_descriptor);

    Ok(())
}

fn driver_gpio() -> Result<(), &'static str> {
    let gpio_descriptor = drivers::DeviceDriverDescriptor::new(&GPIO, Some(post_init_gpio));
    drivers::driver_manager().register_driver(gpio_descriptor);

    Ok(())
}
