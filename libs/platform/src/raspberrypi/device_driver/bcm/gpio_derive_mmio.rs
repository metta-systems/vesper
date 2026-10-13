//! Sketch, not compiled (no `mod` declaration in `bcm/mod.rs`): the BCM GPIO driver
//! from `gpio.rs` rewritten on `derive-mmio` 0.8 with `bitbybit` bitfields, to show
//! the shape of the code and its use. Exact attribute spellings may be off.
//!
//! What changes against the tock-registers version:
//! - The register block is a plain `#[repr(C)]` struct; `#[derive(Mmio)]` generates
//!   `MmioRegisterBlock<'a>`, a handle wrapping a raw pointer. No `&RegisterBlock` is
//!   ever formed, so the compiler cannot invent device reads.
//! - Each register gets `read_x` / `write_x` / `modify_x(closure)` methods.
//!   `PureRead` registers read through `&self`; writes, `modify` and side-effecting
//!   `Read` registers need `&mut self`.
//! - Register contents are `bitbybit` bitfield structs instead of
//!   `register_bitfields!` + `FieldValue`: fields are typed getters and `with_*`
//!   builders, and multi-pin registers become field arrays indexed by pin.

use {
    crate::device_driver::IRQNumber,
    arbitrary_int::{u2, u3},
    bitbybit::{bitenum, bitfield},
    core::marker::PhantomData,
    derive_mmio::Mmio,
    libaddress::{Address, Virtual},
    liblocking::{IRQSafeNullLock, interface::Mutex},
};

/// Generates `pub enums` with no variants for each `ident` passed in.
macro states($($name:ident),*) {
$(pub enum $name {  })*
}

// Possible states for a GPIO pin.
states! {
    Uninitialized, Input, Output, Alt
}

//--------------------------------------------------------------------------------------------------
// Register contents
//--------------------------------------------------------------------------------------------------

/// An alternative GPIO function.
///
/// All eight 3-bit encodings are used, so reading a `FunctionSelect` field
/// returns `Function` directly instead of `Result<Function, u8>`.
#[bitenum(u3, exhaustive = true)]
#[derive(Debug, PartialEq, Eq)]
pub enum Function {
    Input = 0b000,
    Output = 0b001,
    Alt0 = 0b100,
    Alt1 = 0b101,
    Alt2 = 0b110,
    Alt3 = 0b111,
    Alt4 = 0b011,
    Alt5 = 0b010,
}

/// Pull up/down resistor setup.
///
/// `0b11` is reserved, so reading a field of this type returns
/// `Result<PullUpDown, u2>`.
#[bitenum(u2, exhaustive = false)]
#[derive(Debug, PartialEq, Eq)]
pub enum PullUpDown {
    None = 0b00,
    Up = 0b01,
    Down = 0b10,
}

/// GPFSELn: function of ten consecutive pins, three bits each.
#[bitfield(u32, default = 0)]
pub struct FunctionSelect {
    /// `function(n)` / `with_function(n, Function::Alt0)` for pin `10 * bank + n`.
    #[bits(0..=2, stride = 3, rw)]
    function: [Function; 10],
}

/// GPSETn / GPCLRn / GPLEVn: one bit per pin of a 32-pin bank.
#[bitfield(u32, default = 0)]
pub struct PinMask {
    #[bit(0, rw)]
    pin: [bool; 32],
}

/// GPPUD (BCM2837): the pull to apply to pins clocked by `PullUpDownEnableClock`.
#[cfg(board_rpi3)]
#[bitfield(u32, default = 0)]
pub struct PullUpDownRequest {
    #[bits(0..=1, rw)]
    pull: Option<PullUpDown>,
}

/// GPIO_PUP_PDN_CNTRL_REGn (BCM2711): the pull of sixteen pins, two bits each.
#[cfg(board_rpi4)]
#[bitfield(u32, default = 0)]
pub struct PullUpDownControl {
    #[bits(0..=1, stride = 2, rw)]
    pull: [Option<PullUpDown>; 16],
}

//--------------------------------------------------------------------------------------------------
// Register block
//--------------------------------------------------------------------------------------------------

// derive-mmio rejects implicit padding, so reserved gaps are spelled out as
// fields. They get accessors too; nothing calls them.

/// The GPIO register block (BCM2837 ARM Peripherals, section 6.1).
#[cfg(board_rpi3)]
#[derive(Mmio)]
#[repr(C)]
pub struct RegisterBlock {
    #[mmio(offset_bytes(0x00))]
    function_select: [FunctionSelect; 6],
    _reserved_1: u32,
    #[mmio(Write, offset_bytes(0x1c))]
    set_pin: [PinMask; 2],
    _reserved_2: u32,
    #[mmio(Write, offset_bytes(0x28))]
    clear_pin: [PinMask; 2],
    _reserved_3: u32,
    #[mmio(PureRead, offset_bytes(0x34))]
    pin_level: [PinMask; 2],
    _reserved_4: [u32; 22],
    #[mmio(offset_bytes(0x94))]
    pull_up_down: PullUpDownRequest,
    #[mmio(offset_bytes(0x98))]
    pull_up_down_enable_clock: [PinMask; 2],
}

/// The GPIO register block (BCM2711 ARM Peripherals, section 5.2).
#[cfg(board_rpi4)]
#[derive(Mmio)]
#[repr(C)]
pub struct RegisterBlock {
    #[mmio(offset_bytes(0x00))]
    function_select: [FunctionSelect; 6],
    _reserved_1: u32,
    #[mmio(Write, offset_bytes(0x1c))]
    set_pin: [PinMask; 2],
    _reserved_2: u32,
    #[mmio(Write, offset_bytes(0x28))]
    clear_pin: [PinMask; 2],
    _reserved_3: u32,
    #[mmio(PureRead, offset_bytes(0x34))]
    pin_level: [PinMask; 2],
    _reserved_4: [u32; 42],
    #[mmio(offset_bytes(0xe4))]
    pull_up_down_control: [PullUpDownControl; 4],
}

// What the derive generates for the rpi3 block, abridged:
//
// pub struct MmioRegisterBlock<'a> { ptr: *mut RegisterBlock, phantom: PhantomData<&'a mut ()> }
//
// impl RegisterBlock {
//     pub const unsafe fn new_mmio(ptr: *mut RegisterBlock) -> MmioRegisterBlock<'static>;
//     pub const unsafe fn new_mmio_at(addr: usize) -> MmioRegisterBlock<'static>;
// }
//
// impl MmioRegisterBlock<'_> {
//     pub fn read_function_select(&self, index: usize) -> Result<FunctionSelect, OutOfBoundsError>;
//     pub fn write_function_select(&mut self, index: usize, value: FunctionSelect) -> Result<(), OutOfBoundsError>;
//     pub fn modify_function_select(&mut self, index: usize, f: impl FnOnce(FunctionSelect) -> FunctionSelect)
//         -> Result<(), OutOfBoundsError>;
//     pub unsafe fn read_function_select_unchecked(&self, index: usize) -> FunctionSelect;
//     pub const fn len_function_select(&self) -> usize;
//
//     pub fn write_set_pin(&mut self, index: usize, value: PinMask) -> Result<(), OutOfBoundsError>;   // Write only
//     pub fn read_pin_level(&self, index: usize) -> Result<PinMask, OutOfBoundsError>;               // PureRead only
//
//     pub fn read_pull_up_down(&self) -> PullUpDownRequest;
//     pub fn write_pull_up_down(&mut self, value: PullUpDownRequest);
//     pub fn modify_pull_up_down(&mut self, f: impl FnOnce(PullUpDownRequest) -> PullUpDownRequest);
//     pub fn pointer_to_pull_up_down(&mut self) -> *mut PullUpDownRequest;
//     ...
// }

//--------------------------------------------------------------------------------------------------
// Driver
//--------------------------------------------------------------------------------------------------

struct GPIOInner {
    registers: MmioRegisterBlock<'static>,
}

/// Public interface to the GPIO MMIO area
pub struct GPIO {
    inner: IRQSafeNullLock<GPIOInner>,
}

/// Bank and position of `pin` in a register holding `per_register` pins.
/// Pins are at most 53 by construction (`Pin::new`), so every bank index
/// below is in range and the array accessors' `OutOfBoundsError` never fires.
const fn locate(pin: usize, per_register: usize) -> (usize, usize) {
    (pin / per_register, pin % per_register)
}

impl GPIOInner {
    /// # Safety
    ///
    /// `mmio_base_addr` maps the GPIO block as device memory for the rest of
    /// the program, and nothing else accesses it.
    pub const unsafe fn new(mmio_base_addr: Address<Virtual>) -> Self {
        Self {
            // SAFETY: the caller's contract.
            registers: unsafe { RegisterBlock::new_mmio_at(mmio_base_addr.as_usize()) },
        }
    }

    #[cfg(board_rpi3)]
    pub fn power_off(&mut self) {
        use core::time::Duration;

        // The Linux 2837 GPIO driver waits 1 µs between the steps.
        const DELAY: Duration = Duration::from_micros(1);

        // power off gpio pins (but not VCC pins)
        for bank in 0..5 {
            self.registers
                .write_function_select(bank, FunctionSelect::DEFAULT)
                .unwrap();
        }

        self.registers.write_pull_up_down(
            PullUpDownRequest::DEFAULT.with_pull(PullUpDown::None),
        );

        libtime::time::time_manager().spin_for(DELAY);

        let every_pin = PinMask::new_with_raw_value(u32::MAX);
        for bank in 0..2 {
            self.registers
                .write_pull_up_down_enable_clock(bank, every_pin)
                .unwrap();
        }

        libtime::time::time_manager().spin_for(DELAY);

        // flush GPIO setup
        for bank in 0..2 {
            self.registers
                .write_pull_up_down_enable_clock(bank, PinMask::DEFAULT)
                .unwrap();
        }
    }

    #[cfg(board_rpi3)]
    pub fn set_pull_up_down(&mut self, pin: usize, pull: PullUpDown) {
        use core::time::Duration;

        // The Linux 2837 GPIO driver waits 1 µs between the steps.
        const DELAY: Duration = Duration::from_micros(1);

        let (bank, bit) = locate(pin, 32);

        self.registers
            .write_pull_up_down(PullUpDownRequest::DEFAULT.with_pull(pull));

        libtime::time::time_manager().spin_for(DELAY);

        self.registers
            .write_pull_up_down_enable_clock(bank, PinMask::DEFAULT.with_pin(bit, true))
            .unwrap();

        libtime::time::time_manager().spin_for(DELAY);

        self.registers
            .write_pull_up_down(PullUpDownRequest::DEFAULT.with_pull(PullUpDown::None));
        self.registers
            .write_pull_up_down_enable_clock(bank, PinMask::DEFAULT)
            .unwrap();
    }

    #[cfg(board_rpi4)]
    pub fn set_pull_up_down(&mut self, pin: usize, pull: PullUpDown) {
        let (bank, field) = locate(pin, 16);

        self.registers
            .modify_pull_up_down_control(bank, |control| control.with_pull(field, pull))
            .unwrap();
    }

    pub fn to_alt(&mut self, pin: usize, function: Function) {
        let (bank, field) = locate(pin, 10);

        self.registers
            .modify_function_select(bank, |select| select.with_function(field, function))
            .unwrap();
    }

    pub fn set_pin(&mut self, pin: usize) {
        let (bank, bit) = locate(pin, 32);
        // GPSET is write-1-to-set: zero bits leave other pins alone.
        self.registers
            .write_set_pin(bank, PinMask::DEFAULT.with_pin(bit, true))
            .unwrap();
    }

    pub fn clear_pin(&mut self, pin: usize) {
        let (bank, bit) = locate(pin, 32);
        self.registers
            .write_clear_pin(bank, PinMask::DEFAULT.with_pin(bit, true))
            .unwrap();
    }

    /// GPLEV reads have no side effects, so this takes `&self`.
    pub fn get_level(&self, pin: usize) -> Level {
        let (bank, bit) = locate(pin, 32);
        self.registers.read_pin_level(bank).unwrap().pin(bit)
    }

    /// The function `pin` is set to; new with typed register reads.
    pub fn function(&self, pin: usize) -> Function {
        let (bank, field) = locate(pin, 10);
        self.registers.read_function_select(bank).unwrap().function(field)
    }
}

impl GPIO {
    pub const COMPATIBLE: &'static str = "BCM GPIO";

    /// # Safety
    ///
    /// See [`GPIOInner::new`].
    pub const unsafe fn new(mmio_base_addr: Address<Virtual>) -> Self {
        Self {
            inner: IRQSafeNullLock::new(
                // SAFETY: the caller's contract.
                unsafe { GPIOInner::new(mmio_base_addr) },
            ),
        }
    }

    pub fn get_pin(&self, pin: usize) -> Pin<'_, Uninitialized> {
        // SAFETY: Not safe yet
        unsafe { Pin::new(pin, &self.inner) } // todo: expose only inner to avoid unlocked access
    }

    pub fn power_off(&self) {
        self.inner.lock(|inner| inner.power_off());
    }
}

// `Pin`, its typestate transitions and `DeviceDriver` stay exactly as in
// `gpio.rs`: they only call `GPIOInner` through the lock, whose closure already
// receives `&mut GPIOInner`, which is what derive-mmio's writers need.

/// A GPIO pin in state `State`.
#[allow(clippy::struct_field_names)]
pub struct Pin<'outer, State> {
    pin: usize,
    inner: &'outer IRQSafeNullLock<GPIOInner>,
    _state: PhantomData<State>,
}

impl<'outer, State> Pin<'outer, State> {
    #[inline(always)]
    fn transition<NewState>(self) -> Pin<'outer, NewState> {
        Pin {
            pin: self.pin,
            inner: self.inner,
            _state: PhantomData,
        }
    }

    pub fn set_pull_up_down(&self, pull: PullUpDown) {
        self.inner
            .lock(|inner| inner.set_pull_up_down(self.pin, pull));
    }
}

impl<'outer> Pin<'outer, Uninitialized> {
    unsafe fn new(
        pin: usize,
        inner: &'outer IRQSafeNullLock<GPIOInner>,
    ) -> Pin<'outer, Uninitialized> {
        assert!(
            pin <= 53,
            "gpio::Pin::new(): pin {pin} exceeds maximum of 53"
        );

        Pin {
            inner,
            pin,
            _state: PhantomData,
        }
    }

    pub fn into_alt(self, function: Function) -> Pin<'outer, Alt> {
        self.inner.lock(|inner| inner.to_alt(self.pin, function));
        self.transition()
    }

    pub fn into_output(self) -> Pin<'outer, Output> {
        self.into_alt(Function::Output).transition()
    }

    pub fn into_input(self) -> Pin<'outer, Input> {
        self.into_alt(Function::Input).transition()
    }
}

impl Pin<'_, Output> {
    pub fn set(&mut self) {
        self.inner.lock(|inner| inner.set_pin(self.pin));
    }

    pub fn clear(&mut self) {
        self.inner.lock(|inner| inner.clear_pin(self.pin));
    }
}

pub type Level = bool;

impl Pin<'_, Input> {
    pub fn level(&self) -> Level {
        self.inner.lock(|inner| inner.get_level(self.pin))
    }
}

impl libdriver::drivers::interface::DeviceDriver for GPIO {
    type IRQNumberType = IRQNumber;

    fn compatible(&self) -> &'static str {
        Self::COMPATIBLE
    }
}

//--------------------------------------------------------------------------------------------------
// Usage
//--------------------------------------------------------------------------------------------------

// Callers see the same API as with `gpio.rs`:
//
//     let gpio = unsafe { GPIO::new(GPIO_BASE) };
//
//     // PL011 on pins 14/15
//     let _tx = gpio.get_pin(14).into_alt(Function::Alt0);
//     let _rx = gpio.get_pin(15).into_alt(Function::Alt0);
//
//     let mut led = gpio.get_pin(42).into_output();
//     led.set();
//     led.clear();
//
//     let button = gpio.get_pin(17).into_input();
//     button.set_pull_up_down(PullUpDown::Up);
//     if button.level() { /* ... */ }
//
// Inside a driver, register access reads like this, against tock-registers:
//
//     // tock-registers                                            // derive-mmio + bitbybit
//     regs.FunctionSelect[bank].modify(                            regs.modify_function_select(bank, |select| {
//         FieldValue::<u32, ()>::new(0b111, off * 3, f.into()));       select.with_function(field, f)
//                                                                  }).unwrap();
//     regs.SetPin[bank].set(1 << shift);                           regs.write_set_pin(bank, PinMask::DEFAULT.with_pin(bit, true)).unwrap();
//     regs.PinLevel[bank].matches_all(                             regs.read_pin_level(bank).unwrap().pin(bit)
//         FieldValue::<u32, ()>::new(1, off, 1))
