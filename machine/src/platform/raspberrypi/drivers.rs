use {
    super::exception,
    crate::{
        console, drivers,
        exception::{self as generic_exception},
        memory::{self, mmu::MMIODescriptor},
        platform::{device_driver, memory::map::mmio},
    },
    core::{
        mem::MaybeUninit,
        sync::atomic::{AtomicBool, Ordering},
    },
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

    #[cfg(not(feature = "noserial"))]
    driver_uart()?;
    driver_gpio()?;
    driver_interrupt_controller()?;

    INIT_DONE.store(true, Ordering::Relaxed);
    Ok(())
}

/// Minimal code needed to bring up the console in QEMU (for testing only). This is often less steps
/// than on real hardware due to QEMU's abstractions.
#[cfg(test)]
pub fn qemu_bring_up_console() {
    unsafe {
        instantiate_uart().unwrap_or_else(|_| crate::qemu::semihosting::exit_failure());
        console::register_console(PL011_UART.assume_init_ref());
    };
}

//--------------------------------------------------------------------------------------------------
// Global instances
//--------------------------------------------------------------------------------------------------

static mut PL011_UART: MaybeUninit<device_driver::PL011Uart> = MaybeUninit::uninit();
static mut GPIO: MaybeUninit<device_driver::GPIO> = MaybeUninit::uninit();

#[cfg(feature = "rpi3")]
static mut INTERRUPT_CONTROLLER: MaybeUninit<device_driver::InterruptController> =
    MaybeUninit::uninit();

#[cfg(feature = "rpi4")]
static mut INTERRUPT_CONTROLLER: MaybeUninit<device_driver::GICv2> = MaybeUninit::uninit();

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

/// This must be called only after successful init of the memory subsystem.
unsafe fn instantiate_uart() -> Result<(), &'static str> {
    let mmio_descriptor = MMIODescriptor::new(mmio::PL011_UART_BASE, mmio::PL011_UART_SIZE);
    let virt_addr =
        memory::mmu::kernel_map_mmio(device_driver::PL011Uart::COMPATIBLE, &mmio_descriptor)?;

    PL011_UART.write(device_driver::PL011Uart::new(virt_addr));

    Ok(())
}

/// This must be called only after successful init of the PL011 UART driver.
unsafe fn post_init_pl011_uart() -> Result<(), &'static str> {
    console::register_console(PL011_UART.assume_init_ref());
    crate::info!("[0] UART0 is live!");
    Ok(())
}

/// This must be called only after successful init of the memory subsystem.
unsafe fn instantiate_gpio() -> Result<(), &'static str> {
    let mmio_descriptor = MMIODescriptor::new(mmio::GPIO_BASE, mmio::GPIO_SIZE);
    let virt_addr =
        memory::mmu::kernel_map_mmio(device_driver::GPIO::COMPATIBLE, &mmio_descriptor)?;

    GPIO.write(device_driver::GPIO::new(virt_addr));

    Ok(())
}

/// This must be called only after successful init of the GPIO driver.
unsafe fn post_init_gpio() -> Result<(), &'static str> {
    device_driver::PL011Uart::prepare_gpio(GPIO.assume_init_ref());
    Ok(())
}

/// This must be called only after successful init of the memory subsystem.
#[cfg(feature = "rpi3")]
unsafe fn instantiate_interrupt_controller() -> Result<(), &'static str> {
    let periph_mmio_descriptor =
        MMIODescriptor::new(mmio::PERIPHERAL_IC_BASE, mmio::PERIPHERAL_IC_SIZE);
    let periph_virt_addr = memory::mmu::kernel_map_mmio(
        device_driver::InterruptController::COMPATIBLE,
        &periph_mmio_descriptor,
    )?;

    INTERRUPT_CONTROLLER.write(device_driver::InterruptController::new(periph_virt_addr));

    Ok(())
}

/// This must be called only after successful init of the memory subsystem.
#[cfg(feature = "rpi4")]
unsafe fn instantiate_interrupt_controller() -> Result<(), &'static str> {
    let gicd_mmio_descriptor = MMIODescriptor::new(mmio::GICD_BASE, mmio::GICD_SIZE);
    let gicd_virt_addr = memory::mmu::kernel_map_mmio("GICv2 GICD", &gicd_mmio_descriptor)?;

    let gicc_mmio_descriptor = MMIODescriptor::new(mmio::GICC_BASE, mmio::GICC_SIZE);
    let gicc_virt_addr = memory::mmu::kernel_map_mmio("GICV2 GICC", &gicc_mmio_descriptor)?;

    INTERRUPT_CONTROLLER.write(device_driver::GICv2::new(gicd_virt_addr, gicc_virt_addr));

    Ok(())
}

/// This must be called only after successful init of the interrupt controller driver.
unsafe fn post_init_interrupt_controller() -> Result<(), &'static str> {
    generic_exception::asynchronous::register_irq_manager(INTERRUPT_CONTROLLER.assume_init_ref());

    Ok(())
}

/// Function needs to ensure that driver registration happens only after correct instantiation.
unsafe fn driver_uart() -> Result<(), &'static str> {
    instantiate_uart()?;

    let uart_descriptor = drivers::DeviceDriverDescriptor::new(
        PL011_UART.assume_init_ref(),
        Some(post_init_pl011_uart),
        Some(exception::asynchronous::irq_map::PL011_UART),
    );
    drivers::driver_manager().register_driver(uart_descriptor);

    Ok(())
}

/// Function needs to ensure that driver registration happens only after correct instantiation.
unsafe fn driver_gpio() -> Result<(), &'static str> {
    instantiate_gpio()?;

    let gpio_descriptor =
        drivers::DeviceDriverDescriptor::new(GPIO.assume_init_ref(), Some(post_init_gpio), None);
    drivers::driver_manager().register_driver(gpio_descriptor);

    Ok(())
}

/// Function needs to ensure that driver registration happens only after correct instantiation.
unsafe fn driver_interrupt_controller() -> Result<(), &'static str> {
    instantiate_interrupt_controller()?;

    let interrupt_controller_descriptor = drivers::DeviceDriverDescriptor::new(
        INTERRUPT_CONTROLLER.assume_init_ref(),
        Some(post_init_interrupt_controller),
        None,
    );
    drivers::driver_manager().register_driver(interrupt_controller_descriptor);

    Ok(())
}
