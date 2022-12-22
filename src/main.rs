#![no_std]
#![no_main]
#![feature(decl_macro)]
#![feature(format_args_nl)]

use core::{fmt::Write, marker::PhantomData, ops};

#[cfg(not(target_arch = "aarch64"))]
use architecture_not_supported_sorry;

pub mod boot;
pub mod gpio;
pub mod mini_uart;

pub struct BcmHost;

impl BcmHost {
    /// At which address to load the kernel binary.
    pub const fn kernel_load_address() -> u64 {
        0x8_0000
    }

    /// As per <https://www.raspberrypi.org/forums/viewtopic.php?p=1170522#p1170522>
    ///
    pub fn bus2phys(bus: usize) -> usize {
        bus & !0xc000_0000
    }

    pub fn phys2bus(phys: usize) -> usize {
        phys | 0xc000_0000
    }

    /// Name of the hardware device this BcmHost is compiled for.
    pub const fn board_name() -> &'static str {
        "Raspberry Pi 4+"
    }

    /// This returns the ARM-side physical address where peripherals are mapped.
    ///
    pub const fn get_peripheral_address() -> usize {
        0xfe00_0000
    }

    /// This returns the size of the peripherals' space.
    pub const fn get_peripheral_size() -> usize {
        0x0180_0000
    }

    /// This returns the bus address of the SDRAM.
    pub const fn get_sdram_address() -> usize {
        0xc000_0000 // uncached
    }
}

pub struct MMIODerefWrapper<T> {
    base_addr: usize,
    phantom: PhantomData<fn() -> T>,
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl<T> MMIODerefWrapper<T> {
    /// Create an instance.
    ///
    /// # Safety
    ///
    /// Unsafe, duh!
    pub const unsafe fn new(start_addr: usize) -> Self {
        Self {
            base_addr: start_addr,
            phantom: PhantomData,
        }
    }
}

/// Deref to RegisterBlock
///
/// Allows writing
/// ```
/// self.GPPUD.read()
/// ```
/// instead of something along the lines of
/// ```
/// unsafe { (*GPIO::ptr()).GPPUD.read() }
/// ```
impl<T> ops::Deref for MMIODerefWrapper<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*(self.base_addr as *const _) }
    }
}

/// Loop forever in sleep mode.
#[inline]
pub fn endless_sleep() -> ! {
    loop {
        aarch64_cpu::asm::wfe();
    }
}

#[export_name = "main"]
#[inline(always)]
pub unsafe fn __main() -> ! {
    kernel_main();
}

/// Kernel entry point.
pub fn kernel_main() -> ! {
    let gpio = gpio::GPIO::default();
    let mut uart = mini_uart::MiniUart::default().prepare(&gpio);

    uart.write_str("Letsgo!").ok();

    // if you don't comment it out, it works on 08-12 and breaks on 08-13.
    // if you comment this line out on 08-13 everything else starts to work.
    uart.write_fmt(format_args_nl!("Lets {}!", "go")).ok();

    uart.write_str("Lets go 2!").ok();
    uart.flush();
    panic!("Off you go!");
}

#[panic_handler]
pub fn handler(_info: &core::panic::PanicInfo) -> ! {
    crate::endless_sleep()
}
