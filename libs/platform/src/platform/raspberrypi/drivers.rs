use {
    super::exception, // @todo
    crate::platform::{device_driver, exception::asynchronous::IRQNumber},
    core::{
        mem::MaybeUninit,
        sync::atomic::{AtomicBool, Ordering},
    },
    libdriver::drivers::DriverManager,
    libmemory::{mmu::MMIODescriptor, platform::memory::map::mmio},
};

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

/// Return a reference to the global `DriverManager`.
pub fn driver_manager() -> &'static DriverManager<IRQNumber> {
    &DRIVER_MANAGER
}

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
    // SAFETY: You may believe praying will save you.
    unsafe {
        driver_uart()?;
    }
    // SAFETY: You may believe praying will save you.
    unsafe {
        driver_gpio()?;
    }
    // SAFETY: You may believe praying will save you.
    unsafe {
        driver_interrupt_controller()?;
    }

    INIT_DONE.store(true, Ordering::Relaxed);
    Ok(())
}

/// Minimal code needed to bring up the console in QEMU (for testing only). This is often less steps
/// than on real hardware due to QEMU's abstractions.
#[cfg(any(test, feature = "test_build"))]
pub fn qemu_bring_up_console() {
    unsafe {
        instantiate_uart().unwrap_or_else(|_| libqemu::semihosting::exit_failure());
        #[allow(static_mut_refs)]
        libconsole::console::register_console(PL011_UART.assume_init_ref());
    };
}

//--------------------------------------------------------------------------------------------------
// Global instances
//--------------------------------------------------------------------------------------------------

static mut PL011_UART: MaybeUninit<device_driver::PL011Uart> = MaybeUninit::uninit();
static mut GPIO: MaybeUninit<device_driver::GPIO> = MaybeUninit::uninit();

#[cfg(board_rpi3)]
static mut INTERRUPT_CONTROLLER: MaybeUninit<device_driver::InterruptController> =
    MaybeUninit::uninit();

#[cfg(board_rpi4)]
static mut INTERRUPT_CONTROLLER: MaybeUninit<device_driver::GICv2> = MaybeUninit::uninit();

static DRIVER_MANAGER: DriverManager<IRQNumber> = DriverManager::new();

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

/// This must be called only after successful init of the memory subsystem.
unsafe fn instantiate_uart() -> Result<(), &'static str> {
    let mmio_descriptor = MMIODescriptor::new(mmio::PL011_UART_BASE, mmio::PL011_UART_SIZE);
    // SAFETY: You may believe praying will save you.
    let virt_addr = unsafe {
        libmemory::mmu::kernel_map_mmio(device_driver::PL011Uart::COMPATIBLE, &mmio_descriptor)?
    };

    #[allow(static_mut_refs)]
    // SAFETY: You may believe praying will save you.
    unsafe {
        PL011_UART.write(device_driver::PL011Uart::new(virt_addr))
    };

    Ok(())
}

/// This must be called only after successful init of the PL011 UART driver.
#[allow(clippy::unnecessary_wraps)]
unsafe fn post_init_pl011_uart() -> Result<(), &'static str> {
    #[allow(static_mut_refs)]
    libconsole::console::register_console(
        // SAFETY: You may believe praying will save you.
        unsafe { PL011_UART.assume_init_ref() },
    );
    liblog::info!("UART0 is live!");
    Ok(())
}

/// This must be called only after successful init of the memory subsystem.
unsafe fn instantiate_gpio() -> Result<(), &'static str> {
    let mmio_descriptor = MMIODescriptor::new(mmio::GPIO_BASE, mmio::GPIO_SIZE);
    // SAFETY: You may believe praying will save you.
    let virt_addr = unsafe {
        libmemory::mmu::kernel_map_mmio(device_driver::GPIO::COMPATIBLE, &mmio_descriptor)?
    };

    #[allow(static_mut_refs)]
    // SAFETY: You may believe praying will save you.
    unsafe {
        GPIO.write(device_driver::GPIO::new(virt_addr))
    };

    Ok(())
}

/// This must be called only after successful init of the GPIO driver.
#[allow(clippy::unnecessary_wraps)]
unsafe fn post_init_gpio() -> Result<(), &'static str> {
    #[allow(static_mut_refs)]
    device_driver::PL011Uart::prepare_gpio(
        // SAFETY: You may believe praying will save you.
        unsafe { GPIO.assume_init_ref() },
    );
    Ok(())
}

/// This must be called only after successful init of the memory subsystem.
#[cfg(board_rpi3)]
unsafe fn instantiate_interrupt_controller() -> Result<(), &'static str> {
    let periph_mmio_descriptor =
        MMIODescriptor::new(mmio::PERIPHERAL_IC_BASE, mmio::PERIPHERAL_IC_SIZE);
    // SAFETY: You may believe praying will save you.
    let periph_virt_addr = unsafe {
        libmemory::mmu::kernel_map_mmio(
            device_driver::InterruptController::COMPATIBLE,
            &periph_mmio_descriptor,
        )?
    };

    #[allow(static_mut_refs)]
    // SAFETY: You may believe praying will save you.
    unsafe {
        INTERRUPT_CONTROLLER.write(device_driver::InterruptController::new(periph_virt_addr));
    }

    Ok(())
}

/// This must be called only after successful init of the memory subsystem.
#[cfg(board_rpi4)]
unsafe fn instantiate_interrupt_controller() -> Result<(), &'static str> {
    let gic_distr_mmio_descriptor = MMIODescriptor::new(mmio::GICD_BASE, mmio::GICD_SIZE);
    let gic_distr_virt_addr =
        // SAFETY: Not safe!
        unsafe { libmemory::mmu::kernel_map_mmio("GICv2 GICD", &gic_distr_mmio_descriptor)? };

    let gic_ctrlr_mmio_descriptor = MMIODescriptor::new(mmio::GICC_BASE, mmio::GICC_SIZE);
    let gic_ctrlr_virt_addr =
        // SAFETY: Not safe!
        unsafe { libmemory::mmu::kernel_map_mmio("GICV2 GICC", &gic_ctrlr_mmio_descriptor)? };

    #[allow(static_mut_refs)]
    // SAFETY: Not safe!
    unsafe {
        INTERRUPT_CONTROLLER.write(device_driver::GICv2::new(
            gic_distr_virt_addr,
            gic_ctrlr_virt_addr,
        ))
    };

    Ok(())
}

/// This must be called only after successful init of the interrupt controller driver.
#[allow(clippy::unnecessary_wraps)]
unsafe fn post_init_interrupt_controller() -> Result<(), &'static str> {
    #[allow(static_mut_refs)]
    crate::platform::exception::asynchronous::register_irq_manager(
        // SAFETY: You may believe praying will save you.
        unsafe { INTERRUPT_CONTROLLER.assume_init_ref() },
    );

    Ok(())
}

/// Function needs to ensure that driver registration happens only after correct instantiation.
unsafe fn driver_uart() -> Result<(), &'static str> {
    // SAFETY: You may believe praying will save you.
    unsafe {
        instantiate_uart()?;
    }

    let uart_descriptor = libdriver::drivers::DeviceDriverDescriptor::new(
        #[allow(static_mut_refs)]
        // SAFETY: You may believe praying will save you.
        unsafe {
            PL011_UART.assume_init_ref()
        },
        Some(post_init_pl011_uart),
        Some(exception::asynchronous::irq_map::PL011_UART),
    );
    driver_manager().register_driver(uart_descriptor);

    Ok(())
}

/// Function needs to ensure that driver registration happens only after correct instantiation.
unsafe fn driver_gpio() -> Result<(), &'static str> {
    // SAFETY: You may believe praying will save you.
    unsafe {
        instantiate_gpio()?;
    }

    let gpio_descriptor = libdriver::drivers::DeviceDriverDescriptor::new(
        #[allow(static_mut_refs)]
        // SAFETY: You may believe praying will save you.
        unsafe {
            GPIO.assume_init_ref()
        },
        Some(post_init_gpio),
        None,
    );
    driver_manager().register_driver(gpio_descriptor);

    Ok(())
}

/// Function needs to ensure that driver registration happens only after correct instantiation.
unsafe fn driver_interrupt_controller() -> Result<(), &'static str> {
    // SAFETY: You may believe praying will save you.
    unsafe {
        instantiate_interrupt_controller()?;
    }

    let interrupt_controller_descriptor = libdriver::drivers::DeviceDriverDescriptor::new(
        #[allow(static_mut_refs)]
        // SAFETY: You may believe praying will save you.
        unsafe {
            INTERRUPT_CONTROLLER.assume_init_ref()
        },
        Some(post_init_interrupt_controller),
        None,
    );
    driver_manager().register_driver(interrupt_controller_descriptor);

    Ok(())
}
