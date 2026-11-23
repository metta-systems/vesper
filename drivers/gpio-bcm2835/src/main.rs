// Host builds only check the interface implementation; the component itself
// runs on Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! The gpio component: the one owner of the BCM2835 or BCM2711 GPIO block,
//! behind `gpio_v1` (see `interfaces/gpio_v1.if`).
//!
//! Kickstart loads it for the `brcm,bcm2835-gpio` or `brcm,bcm2711-gpio`
//! node; the matched string tells the pin count and how pulls are set
//! ([`gpio_bcm2835::Chip`]). A driver whose device
//! needs its pins routed imports `gpio_v1` and switches them when it starts.
//! A client drives the pins it owns only: its key's badge is the pin set its
//! node names (`libdevice_driver::PinSet`), and every function refuses a pin
//! outside it. Calls from different clients take turns, since switching one
//! pin rewrites the register it shares with nine others, and a pull change
//! takes the block's one pull control register through several steps.

use gpio::gpio_v1::{Exception, GpioV1, PinFunction, PinPull};

/// The block.
struct Gpio;

/// `pin` if `chip()`'s block has it and the key with `badge` owns it.
fn owned(badge: u64, pin: u32) -> Result<u32, Exception> {
    gpio_bcm2835::place(chip(), pin).ok_or(Exception::InvalidPin { pin })?;
    if owns(badge, pin) {
        Ok(pin)
    } else {
        Err(Exception::NotOwned { pin })
    }
}

impl GpioV1 for Gpio {
    #[cfg(target_os = "vesper")]
    fn enter(_badge: u64) {
        component::record_return_key();
    }

    fn set_function(badge: u64, pin: u32, function: PinFunction) -> Result<(), Exception> {
        let pin = owned(badge, pin)?;
        let (register, slot) =
            gpio_bcm2835::place(chip(), pin).ok_or(Exception::InvalidPin { pin })?;
        set_function(register, slot, function);
        Ok(())
    }

    fn set_pull(badge: u64, pin: u32, pull: PinPull) -> Result<(), Exception> {
        set_pull(owned(badge, pin)?, pull);
        Ok(())
    }

    fn set_level(badge: u64, pin: u32, high: bool) -> Result<(), Exception> {
        set_level(owned(badge, pin)?, high);
        Ok(())
    }
}

gpio::gpio_v1_export!(Gpio);

#[cfg(target_os = "vesper")]
use component::{chip, owns, set_function, set_level, set_pull};

#[cfg(not(target_os = "vesper"))]
const fn chip() -> gpio_bcm2835::Chip {
    gpio_bcm2835::Chip::Bcm2835
}

#[cfg(not(target_os = "vesper"))]
const fn owns(_badge: u64, _pin: u32) -> bool {
    true
}

#[cfg(not(target_os = "vesper"))]
fn set_function(_register: usize, _slot: usize, _function: PinFunction) {}

#[cfg(not(target_os = "vesper"))]
fn set_pull(_pin: u32, _pull: PinPull) {}

#[cfg(not(target_os = "vesper"))]
fn set_level(_pin: u32, _high: bool) {}

#[cfg(target_os = "vesper")]
mod component {
    use {
        super::{PinFunction, PinPull},
        core::sync::atomic::{AtomicU64, Ordering},
        gpio_bcm2835::{Chip, MmioRegisterBlock, RegisterBlock, apply_pull, select},
        libdevice_driver::{
            INIT_VA, PinSet, Turnstile, device_address, driver_compatible, driver_init,
            map_device_pages,
        },
        libobject::{
            EventCountKey, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
            export::init_return_key, thread::ThreadReturnKey,
        },
    };

    libuser::entry!(main);

    driver_compatible!("brcm,bcm2835-gpio", "brcm,bcm2711-gpio");

    /// The block the node is, from the compatible string it matched.
    pub fn chip() -> Chip {
        Chip::from_compatible_index(init().compatible_index)
            .expect("gpio: one of the driver's compatible strings")
    }

    /// Whether the key with `badge` owns `pin`: Kickstart badged it with the
    /// pins the client's node names. A key without a pin set owns nothing.
    pub fn owns(badge: u64, pin: u32) -> bool {
        PinSet::from_badge(badge).is_some_and(|pins| pins.contains(pin))
    }

    /// Where the block's registers are mapped (zero until set up).
    static REGISTERS_VA: AtomicU64 = AtomicU64::new(0);

    /// Admits one caller at a time to the registers.
    static TURNSTILE: Turnstile = Turnstile::new();

    /// The wait between the steps of a pull change: the manual asks for 150
    /// cycles, and Linux waits 1 µs.
    const PULL_STEP_NS: u64 = 1_000;

    fn init() -> &'static libdevice_driver::DriverInit {
        // SAFETY: Kickstart maps every driver's init record at `INIT_VA`; it
        // is read in this component's own `AddressSpace` only.
        unsafe { driver_init(INIT_VA) }
    }

    /// Record this component's `Thread.Return` key for its exports.
    pub fn record_return_key() {
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            init().return_key,
        )));
    }

    /// The block's registers.
    ///
    /// # Safety
    ///
    /// The caller holds a turn of [`TURNSTILE`], so no other caller touches
    /// them.
    unsafe fn registers() -> MmioRegisterBlock<'static> {
        // SAFETY: `main` mapped the registers for good before the first call.
        unsafe {
            RegisterBlock::new_mmio(REGISTERS_VA.load(Ordering::Acquire) as *mut RegisterBlock)
        }
    }

    /// Give `pin` (a pin of the block) the pull `pull`.
    pub fn set_pull(pin: u32, pull: PinPull) {
        let _turn = TURNSTILE.enter(init());
        // SAFETY: this caller holds the turn.
        let mut registers = unsafe { registers() };
        apply_pull(&mut registers, chip(), pin, pull, |_| {
            let until = libuser::clock::now_ns() + PULL_STEP_NS;
            while libuser::clock::now_ns() < until {
                core::hint::spin_loop();
            }
        })
        .expect("gpio: a pin of the block");
    }

    /// Drive `pin` (a pin of the block) high or low. One write to `GPSETn` or
    /// `GPCLRn`, which touches no other pin; the turn keeps it apart from a
    /// pull change all the same.
    pub fn set_level(pin: u32, high: bool) {
        let _turn = TURNSTILE.enter(init());
        // SAFETY: this caller holds the turn.
        let mut registers = unsafe { registers() };
        gpio_bcm2835::set_level(&mut registers, chip(), pin, high)
            .expect("gpio: a pin of the block");
    }

    /// Switch the pin at `slot` of function select register `register`.
    pub fn set_function(register: usize, slot: usize, function: PinFunction) {
        let _turn = TURNSTILE.enter(init());
        // SAFETY: this caller holds the turn.
        let mut registers = unsafe { registers() };
        let current = registers
            .read_function_select(register)
            .expect("gpio: a register of the block");
        registers
            .write_function_select(register, select(current, slot, function))
            .expect("gpio: a register of the block");
    }

    /// Map the block's registers, set up the turnstile, then park: everything
    /// else happens in calls.
    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let slot = map_device_pages(init)
            .unwrap_or_else(|error| panic!("gpio: mapping registers failed: {:?}", error.code()));
        REGISTERS_VA.store(
            device_address(init, init.regions()[0].start).expect("gpio: registers mapped"),
            Ordering::Release,
        );
        let table = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let turns = UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::EVENT_COUNT,
                0,
                0,
                1,
                &table,
                u32::try_from(slot).unwrap_or(u32::MAX),
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("gpio: Retype failed: {:?}", error.code()));
        TURNSTILE.open(turns);
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| panic!("gpio: ready advance failed: {:?}", error.code()));
        let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
        loop {
            let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}
