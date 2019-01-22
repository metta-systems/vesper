// See BCM2835-ARM-Peripherals.pdf
// See https://www.raspberrypi.org/forums/viewtopic.php?t=186090 for more details.

pub struct BcmHost;

impl BcmHost {
    // As per https://www.raspberrypi.org/documentation/hardware/raspberrypi/peripheral_addresses.md
    /// This returns the ARM-side physical address where peripherals are mapped.
    pub const fn get_peripheral_address() -> u32 {
        0x3f00_0000
    }

    /// This returns the size of the peripherals' space.
    pub const fn get_peripheral_size() -> usize {
        0x0100_0000
    }

    /// This returns the bus address of the SDRAM.
    pub const fn get_sdram_address() -> usize {
        0xC000_0000 // uncached
    }
}
