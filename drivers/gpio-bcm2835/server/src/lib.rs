#![no_std]

//! The `gpio_v1` interface, generated from `drivers/gpio-bcm2835/interfaces/gpio_v1.if`:
//! its types, the [`gpio_v1::GpioV1`] trait the gpio component implements, and
//! `gpio_v1_export!`, which exports an implementation's functions as PPC entries.

include!(concat!(env!("OUT_DIR"), "/gpio_v1.rs"));

impl gpio_v1::PinFunction {
    /// Every function, in the order of their function select codes.
    const BY_SELECT_CODE: [Self; 8] = [
        Self::Input,
        Self::Output,
        Self::Alt5,
        Self::Alt4,
        Self::Alt0,
        Self::Alt1,
        Self::Alt2,
        Self::Alt3,
    ];

    /// The function whose function select code is `code`, as the device
    /// tree's `brcm,function` gives it, or `None` when `code` is none (BCM2835
    /// ARM Peripherals, section 6.1: ALT4 is 3 and ALT5 is 2).
    #[must_use]
    pub fn from_select_code(code: u32) -> Option<Self> {
        Self::BY_SELECT_CODE
            .get(usize::try_from(code).ok()?)
            .copied()
    }

    /// This function's function select code.
    #[must_use]
    pub fn select_code(self) -> u8 {
        Self::BY_SELECT_CODE
            .iter()
            .position(|function| *function == self)
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or_default()
    }
}

impl gpio_v1::PinPull {
    /// Every pull, in the order of their codes.
    const BY_CODE: [Self; 3] = [Self::Off, Self::Down, Self::Up];

    /// The pull whose code is `code`, as the device tree's `brcm,pull` gives
    /// it, or `None` when `code` is none (BCM2835 ARM Peripherals, section
    /// 6.1, `GPPUD`: 0 off, 1 down, 2 up).
    #[must_use]
    pub fn from_code(code: u32) -> Option<Self> {
        Self::BY_CODE.get(usize::try_from(code).ok()?).copied()
    }

    /// This pull's code.
    #[must_use]
    pub fn code(self) -> u8 {
        Self::BY_CODE
            .iter()
            .position(|pull| *pull == self)
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or_default()
    }
}
