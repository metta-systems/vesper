use {
    crate::{
        console, drivers,
        platform::{device_driver, memory::map::mmio},
    },
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

/// Minimal code needed to bring up the console in QEMU (for testing only). This is often less steps
/// than on real hardware due to QEMU's abstractions.
#[cfg(test)]
pub fn qemu_bring_up_console() {
    console::register_console(&PL011_UART);
}

//--------------------------------------------------------------------------------------------------
// Global instances
//--------------------------------------------------------------------------------------------------

static MINI_UART: device_driver::MiniUart =
    unsafe { device_driver::MiniUart::new(device_driver::UART1_BASE) };
static PL011_UART: device_driver::PL011Uart =
    unsafe { device_driver::PL011Uart::new(device_driver::UART0_BASE) };
static GPIO: device_driver::GPIO = unsafe { device_driver::GPIO::new(device_driver::GPIO_BASE) };

#[cfg(feature = "rpi3")]
static INTERRUPT_CONTROLLER: device_driver::InterruptController =
    unsafe { device_driver::InterruptController::new(mmio::PERIPHERAL_IC_START) };

#[cfg(feature = "rpi4")]
static INTERRUPT_CONTROLLER: device_driver::GICv2 =
    unsafe { device_driver::GICv2::new(mmio::GICD_START, mmio::GICC_START) };

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

/// This must be called only after successful init of the Mini UART driver.
fn post_init_mini_uart() -> Result<(), &'static str> {
    console::register_console(&MINI_UART);
    crate::info!("[0] MiniUART is live!");
    Ok(())
}

/// This must be called only after successful init of the PL011 UART driver.
fn post_init_pl011_uart() -> Result<(), &'static str> {
    console::register_console(&PL011_UART);
    crate::info!("[0] UART0 is live!");
    Ok(())
}

// This must be called only after successful init of the GPIO driver.
fn post_init_gpio() -> Result<(), &'static str> {
    // device_driver::MiniUart::prepare_gpio(&GPIO);
    device_driver::PL011Uart::prepare_gpio(&GPIO);
    Ok(())
}

fn driver_uart() -> Result<(), &'static str> {
    // let uart_descriptor =
    //     drivers::DeviceDriverDescriptor::new(&MINI_UART, Some(post_init_mini_uart));
    // drivers::driver_manager().register_driver(uart_descriptor);

    let uart_descriptor =
        drivers::DeviceDriverDescriptor::new(&PL011_UART, Some(post_init_pl011_uart), None);
    drivers::driver_manager().register_driver(uart_descriptor);

    Ok(())
}

fn driver_gpio() -> Result<(), &'static str> {
    let gpio_descriptor = drivers::DeviceDriverDescriptor::new(&GPIO, Some(post_init_gpio), None);
    drivers::driver_manager().register_driver(gpio_descriptor);

    Ok(())
}
