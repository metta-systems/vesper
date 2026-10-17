//! Sketch, not compiled (no `mod` declaration in `bcm/mod.rs`): the BCM GPIO driver
//! from `gpio.rs` rewritten on the proposed pointer-based `libmmio`, which keeps
//! tock-registers' bitfields and interface traits but never forms a reference to
//! device memory. `libmmio` does not exist in this form yet; this shows the shape
//! of the code and its use.
//!
//! What changes against the tock-registers version:
//! - `register_block!` takes the same rows as `register_structs!`, plus a handle
//!   name. It generates the `#[repr(C)]` layout (offset-checked by
//!   `register_structs!`) and `Registers<'a>`, a handle wrapping a raw pointer.
//! - Registers are reached through accessor methods instead of struct fields:
//!   `name()` takes `&self` and exists when reads have no side effects;
//!   `name_mut()` takes `&mut self` and exists when the register can be written
//!   or read with side effects. The handles they return implement tock's
//!   `Readable` / `Writeable`, so `read`, `is_set`, `matches_all`, `read_as_enum`,
//!   `write`, `modify` are unchanged.
//! - Cell types state whether reads are pure: `ReadPure`, `ReadPureWrite`,
//!   `ReadOnly` / `ReadWrite` (side-effecting read), `WriteOnly`.
//! - Arrays of registers return a `RegisterArray` handle; `get(index)` returns
//!   `Option<Register>`.

use {
    crate::device_driver::IRQNumber,
    core::{marker::PhantomData, ptr::NonNull},
    libaddress::{Address, Virtual},
    liblocking::{IRQSafeNullLock, interface::Mutex},
    libmmio::{
        register_block,
        registers::{ReadPure, ReadPureWrite, WriteOnly},
    },
    tock_registers::{
        fields::Field,
        interfaces::{ReadWriteable, Readable, Writeable},
        register_bitfields,
    },
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

// Plain tock-registers bitfields: nothing here depends on how registers are accessed.
// Registers holding one field per pin declare that field once, at pin 0;
// `function_field`, `pin_field` and `pull_field` shift it to a given pin.

register_bitfields! {
    u32,

    /// GPFSELn: function of ten consecutive pins, three bits each.
    pub GPFSEL [
        /// Function of the bank's first pin
        FSEL OFFSET(0) NUMBITS(3) [
            Input = 0b000,
            Output = 0b001,
            Alt0 = 0b100,
            Alt1 = 0b101,
            Alt2 = 0b110,
            Alt3 = 0b111,
            Alt4 = 0b011,
            Alt5 = 0b010
        ]
    ],

    /// GPSETn / GPCLRn / GPLEVn / GPPUDCLKn: one bit per pin of a 32-pin bank.
    pub PINS [
        /// The bank's first pin
        PIN OFFSET(0) NUMBITS(1) []
    ]
}

#[cfg(board_rpi3)]
register_bitfields! {
    u32,

    /// GPPUD (BCM2837): the pull applied to pins clocked by GPPUDCLKn.
    pub GPPUD [
        PUD OFFSET(0) NUMBITS(2) [
            Off = 0b00,
            PullDown = 0b01,
            PullUp = 0b10
        ]
    ]
}

#[cfg(board_rpi4)]
register_bitfields! {
    u32,

    /// GPIO_PUP_PDN_CNTRL_REGn (BCM2711): the pull of sixteen pins, two bits each.
    pub GPIO_PUP_PDN_CNTRL [
        /// Pull of the bank's first pin
        PULL OFFSET(0) NUMBITS(2) [
            None = 0b00,
            Up = 0b01,
            Down = 0b10
        ]
    ]
}

/// An alternative GPIO function: the enum `register_bitfields!` generated for `FSEL`.
pub use GPFSEL::FSEL::Value as Function;

/// Pull up/down resistor setup.
#[derive(PartialEq, Eq, Copy, Clone)]
pub enum PullUpDown {
    None,
    Up,
    Down,
}

/// The `FSEL` field of pin `index` within its bank (`0..10`).
fn function_field(index: usize) -> Field<u32, GPFSEL::Register> {
    Field::new(GPFSEL::FSEL.mask, index * 3)
}

/// The bit of pin `index` within its bank (`0..32`).
fn pin_field(index: usize) -> Field<u32, PINS::Register> {
    Field::new(PINS::PIN.mask, index)
}

/// The pull field of pin `index` within its bank (`0..16`).
#[cfg(board_rpi4)]
fn pull_field(index: usize) -> Field<u32, GPIO_PUP_PDN_CNTRL::Register> {
    Field::new(GPIO_PUP_PDN_CNTRL::PULL.mask, index * 2)
}

//--------------------------------------------------------------------------------------------------
// Register block
//--------------------------------------------------------------------------------------------------

#[cfg(board_rpi3)]
register_block! {
    /// The GPIO register block (BCM2837 ARM Peripherals, section 6.1).
    RegisterBlock => Registers {
        (0x00 => function_select: [ReadPureWrite<u32, GPFSEL::Register>; 6]),
        (0x18 => _reserved_1),
        (0x1c => set_pin: [WriteOnly<u32, PINS::Register>; 2]),
        (0x24 => _reserved_2),
        (0x28 => clear_pin: [WriteOnly<u32, PINS::Register>; 2]),
        (0x30 => _reserved_3),
        (0x34 => pin_level: [ReadPure<u32, PINS::Register>; 2]),
        (0x3c => _reserved_4),
        (0x94 => pull_up_down: ReadPureWrite<u32, GPPUD::Register>),
        (0x98 => pull_up_down_enable_clock: [ReadPureWrite<u32, PINS::Register>; 2]),
        (0xa0 => @END),
    }
}

#[cfg(board_rpi4)]
register_block! {
    /// The GPIO register block (BCM2711 ARM Peripherals, section 5.2).
    RegisterBlock => Registers {
        (0x00 => function_select: [ReadPureWrite<u32, GPFSEL::Register>; 6]),
        (0x18 => _reserved_1),
        (0x1c => set_pin: [WriteOnly<u32, PINS::Register>; 2]),
        (0x24 => _reserved_2),
        (0x28 => clear_pin: [WriteOnly<u32, PINS::Register>; 2]),
        (0x30 => _reserved_3),
        (0x34 => pin_level: [ReadPure<u32, PINS::Register>; 2]),
        (0x3c => _reserved_4),
        (0xe4 => pull_up_down_control: [ReadPureWrite<u32, GPIO_PUP_PDN_CNTRL::Register>; 4]),
        (0xf4 => @END),
    }
}

// What the macro generates for the rpi3 block, abridged. `Pure` handles are
// `Copy + Sync` and only `Readable`; `Exclusive` handles are neither `Clone` nor
// `Sync` and are `Readable` / `Writeable` as the cell allows.
//
// #[repr(C)] pub struct RegisterBlock { /* inert cells, laid out by register_structs! */ }
//
// pub struct Registers<'a>(libmmio::Mmio<'a, RegisterBlock>);
//
// impl<'a> Registers<'a> {
//     pub const unsafe fn new(base: NonNull<RegisterBlock>) -> Self;
//     pub fn shared(&self) -> libmmio::SharedMmio<'_, RegisterBlock>;
//
//     // ReadPureWrite: both
//     pub fn function_select(&self)
//         -> RegisterArray<'_, u32, GPFSEL::Register, Pure, 6>;
//     pub fn function_select_mut(&mut self)
//         -> RegisterArray<'_, u32, GPFSEL::Register, Exclusive, 6>;
//     // WriteOnly: `_mut` only
//     pub fn set_pin_mut(&mut self) -> RegisterArray<'_, u32, PINS::Register, Exclusive, 2>;
//     // ReadPure: shared only
//     pub fn pin_level(&self) -> RegisterArray<'_, u32, PINS::Register, Pure, 2>;
//
//     pub fn pull_up_down(&self) -> Register<'_, u32, GPPUD::Register, Pure>;
//     pub fn pull_up_down_mut(&mut self) -> Register<'_, u32, GPPUD::Register, Exclusive>;
//     ...
// }
//
// impl RegisterArray<'_, T, R, Mode, N> {
//     pub fn get(&mut self, index: usize) -> Option<Register<'_, T, R, Mode>>;  // `&self` for Pure
//     pub fn iter_mut(&mut self) -> impl Iterator<Item = Register<'_, T, R, Mode>>;
//     pub const fn len(&self) -> usize;
// }

//--------------------------------------------------------------------------------------------------
// Driver
//--------------------------------------------------------------------------------------------------

struct GPIOInner {
    registers: Registers<'static>,
}

/// Public interface to the GPIO MMIO area
pub struct GPIO {
    inner: IRQSafeNullLock<GPIOInner>,
}

/// Bank and position of `pin` in a register holding `per_register` pins.
/// Pins are at most 53 by construction (`Pin::new`), so every bank index
/// below is in range and `get(bank)` never returns `None`.
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
            // SAFETY: the caller's contract; a mapped address is not null.
            registers: unsafe {
                Registers::new(NonNull::new_unchecked(mmio_base_addr.as_usize() as *mut _))
            },
        }
    }

    #[cfg(board_rpi3)]
    pub fn power_off(&mut self) {
        use core::time::Duration;

        // The Linux 2837 GPIO driver waits 1 µs between the steps.
        const DELAY: Duration = Duration::from_micros(1);

        // power off gpio pins (but not VCC pins)
        for register in self.registers.function_select_mut().iter_mut().take(5) {
            register.set(0);
        }

        self.registers.pull_up_down_mut().write(GPPUD::PUD::Off);

        libtime::time::time_manager().spin_for(DELAY);

        for register in self.registers.pull_up_down_enable_clock_mut().iter_mut() {
            register.set(u32::MAX);
        }

        libtime::time::time_manager().spin_for(DELAY);

        // flush GPIO setup
        for register in self.registers.pull_up_down_enable_clock_mut().iter_mut() {
            register.set(0);
        }
    }

    #[cfg(board_rpi4)]
    pub fn power_off(&mut self) {
        todo!()
    }

    #[cfg(board_rpi3)]
    pub fn set_pull_up_down(&mut self, pin: usize, pull: PullUpDown) {
        use core::time::Duration;

        // The Linux 2837 GPIO driver waits 1 µs between the steps.
        const DELAY: Duration = Duration::from_micros(1);

        let (bank, bit) = locate(pin, 32);

        self.registers.pull_up_down_mut().write(match pull {
            PullUpDown::None => GPPUD::PUD::Off,
            PullUpDown::Up => GPPUD::PUD::PullUp,
            PullUpDown::Down => GPPUD::PUD::PullDown,
        });

        libtime::time::time_manager().spin_for(DELAY);

        // Each handle borrows `self.registers` mutably, so it is taken again
        // per step rather than held across the GPPUD write below.
        self.registers
            .pull_up_down_enable_clock_mut()
            .get(bank)
            .unwrap()
            .write(pin_field(bit).val(1));

        libtime::time::time_manager().spin_for(DELAY);

        self.registers.pull_up_down_mut().write(GPPUD::PUD::Off);
        self.registers
            .pull_up_down_enable_clock_mut()
            .get(bank)
            .unwrap()
            .set(0);
    }

    #[cfg(board_rpi4)]
    pub fn set_pull_up_down(&mut self, pin: usize, pull: PullUpDown) {
        use GPIO_PUP_PDN_CNTRL::PULL;

        let (bank, index) = locate(pin, 16);
        let value = match pull {
            PullUpDown::None => PULL::Value::None,
            PullUpDown::Up => PULL::Value::Up,
            PullUpDown::Down => PULL::Value::Down,
        };

        self.registers
            .pull_up_down_control_mut()
            .get(bank)
            .unwrap()
            .modify(pull_field(index).val(value as u32));
    }

    pub fn to_alt(&mut self, pin: usize, function: Function) {
        let (bank, index) = locate(pin, 10);

        self.registers
            .function_select_mut()
            .get(bank)
            .unwrap()
            .modify(function_field(index).val(function as u32));
    }

    pub fn set_pin(&mut self, pin: usize) {
        let (bank, bit) = locate(pin, 32);
        // GPSET is write-1-to-set: zero bits leave other pins alone.
        self.registers
            .set_pin_mut()
            .get(bank)
            .unwrap()
            .write(pin_field(bit).val(1));
    }

    pub fn clear_pin(&mut self, pin: usize) {
        let (bank, bit) = locate(pin, 32);
        self.registers
            .clear_pin_mut()
            .get(bank)
            .unwrap()
            .write(pin_field(bit).val(1));
    }

    /// GPLEV reads have no side effects, so this takes `&self`.
    pub fn get_level(&self, pin: usize) -> Level {
        let (bank, bit) = locate(pin, 32);
        self.registers.pin_level().get(bank).unwrap().is_set(pin_field(bit))
    }

    /// The function `pin` is set to, also through `&self`.
    pub fn function(&self, pin: usize) -> Option<Function> {
        let (bank, index) = locate(pin, 10);
        self.registers
            .function_select()
            .get(bank)
            .unwrap()
            .read_as_enum(function_field(index))
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
// receives `&mut GPIOInner`, which is what the `_mut` accessors need.

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
// Inside a driver, register access against today's tock-registers:
//
//     // tock-registers (&RegisterBlock)                    // libmmio (pointer handles)
//     regs.FunctionSelect[bank].modify(                     regs.function_select_mut().get(bank).unwrap()
//         FieldValue::<u32, ()>::new(0b111, off * 3,            .modify(function_field(index).val(f as u32));
//                                    f.into()));
//     regs.SetPin[bank].set(1 << shift);                    regs.set_pin_mut().get(bank).unwrap()
//                                                               .write(pin_field(bit).val(1));
//     regs.PinLevel[bank].matches_all(                      regs.pin_level().get(bank).unwrap()
//         FieldValue::<u32, ()>::new(1, off, 1))                .is_set(pin_field(bit))
//
// For a single register with named fields the code is identical except for the
// accessor call, for example the PL011:
//
//     registers.Control.modify(CR::UARTEN::Enabled);        registers.control_mut().modify(CR::UARTEN::Enabled);
//     registers.Flag.is_set(FR::BUSY)                       registers.flag().is_set(FR::BUSY)
//
// What the types now reject at compile time:
//
//     self.registers.pin_level_mut()          // no such method: GPLEV is ReadPure
//     self.registers.set_pin().get(0)         // no such method: GPSET is WriteOnly, needs `_mut`
//     fn peek(gpio: &GPIOInner) { gpio.registers.function_select_mut() }  // needs `&mut`
