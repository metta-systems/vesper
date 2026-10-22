// Host builds only check the interface implementation; the component itself
// runs on Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! The gpio component: the one owner of the BCM2835 GPIO block, behind
//! `gpio_v1` (see `interfaces/gpio_v1.if`).
//!
//! Kickstart loads it for the `brcm,bcm2835-gpio` node. A driver whose device
//! needs its pins routed imports `gpio_v1` and switches them when it starts.
//! Calls from different clients take turns, since switching one pin rewrites
//! the register it shares with nine others, and a pull change takes the
//! block's one pull control register through several steps.

use gpio::gpio_v1::{Exception, GpioV1, PinFunction, PinPull};

/// The block.
struct Gpio;

impl GpioV1 for Gpio {
    #[cfg(target_os = "vesper")]
    fn enter(_badge: u64) {
        component::record_return_key();
    }

    fn set_function(pin: u32, function: PinFunction) -> Result<(), Exception> {
        let (register, slot) = gpio_bcm2835::place(pin).ok_or(Exception::InvalidPin { pin })?;
        set_function(register, slot, function);
        Ok(())
    }

    fn set_pull(pin: u32, pull: PinPull) -> Result<(), Exception> {
        gpio_bcm2835::place(pin).ok_or(Exception::InvalidPin { pin })?;
        set_pull(pin, pull);
        Ok(())
    }
}

gpio::gpio_v1_export!(Gpio);

#[cfg(target_os = "vesper")]
use component::{set_function, set_pull};

#[cfg(not(target_os = "vesper"))]
fn set_function(_register: usize, _slot: usize, _function: PinFunction) {}

#[cfg(not(target_os = "vesper"))]
fn set_pull(_pin: u32, _pull: PinPull) {}

#[cfg(target_os = "vesper")]
mod component {
    use {
        super::{PinFunction, PinPull},
        core::sync::atomic::{AtomicU64, Ordering},
        gpio_bcm2835::{MmioRegisterBlock, RegisterBlock, apply_pull, select},
        libdevice_driver::{
            INIT_VA, Turnstile, device_address, driver_compatible, driver_init, map_device_pages,
        },
        libobject::{
            EventCountKey, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
            export::init_return_key, thread::ThreadReturnKey,
        },
    };

    libuser::entry!(main);

    driver_compatible!("brcm,bcm2835-gpio");

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
        apply_pull(&mut registers, pin, pull, |_| {
            let until = libuser::clock::now_ns() + PULL_STEP_NS;
            while libuser::clock::now_ns() < until {
                core::hint::spin_loop();
            }
        })
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
