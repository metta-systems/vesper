// The component runs on Vesper only; see the host stub below.
#![cfg_attr(target_os = "vesper", no_std, no_main)]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(all(target_os = "vesper", feature = "qemu"), feature(format_args_nl))]

//! The cprman component: the one owner of the BCM2835 clock manager, behind
//! `clock_v1` (see `interfaces/clock_v1.if`).
//!
//! Kickstart loads it for the `brcm,bcm2835-cprman` node. A driver whose
//! device runs from one of the manager's clocks imports `clock_v1` through its
//! node's `clocks` property and sets the clock's rate when it starts. Only the
//! PWM clock is served so far; the others (UART, SD card, the GP clocks) run
//! as the firmware set them. The manager's pages also hold the PLL controls,
//! which this component never writes.
//!
//! Calls take turns: changing a generator is a sequence of writes with a wait
//! for its `BUSY` flag in the middle (`cprman_bcm2835::program`).
//!
//! The component cannot yet tell which clocks a caller's node names: Kickstart
//! passes the link's phandle, not the clock number after it. Serving only the
//! PWM clock keeps that harmless until it does.

// The interface crates link the syscall layer, which builds for AArch64 only,
// so a host build has nothing but this stub: the library and its host tests
// (`tests/cprman.rs`) carry the logic.
#[cfg(not(target_os = "vesper"))]
fn main() {}

#[cfg(target_os = "vesper")]
mod component {
    use {
        clock::clock_v1::{ClockV1, Exception},
        core::sync::atomic::{AtomicU64, Ordering},
        cprman_bcm2835::{
            Clock, Generator, MmioRegisterBlock, PwmGenerator, RegisterBlock, Setting, choose,
            program, starting, stop,
        },
        libdevice_driver::{
            INIT_VA, Turnstile, device_address, driver_compatible, driver_init, map_device_pages,
        },
        libobject::{
            EventCountKey, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
            export::init_return_key, thread::ThreadReturnKey,
        },
        libuser::semihosting as semi,
    };

    libuser::entry!(main);

    driver_compatible!("brcm,bcm2835-cprman");

    /// The clock manager.
    struct Cprman;

    impl ClockV1 for Cprman {
        fn enter(_badge: u64) {
            record_return_key();
        }

        fn set_rate(clock: u32, hz: u32) -> Result<u32, Exception> {
            let generator = Clock::from_binding(clock).ok_or(Exception::UnknownClock { clock })?;
            let setting = choose(hz).ok_or(Exception::UnreachableRate { hz })?;
            set_rate(generator, setting)?;
            Ok(setting.hz())
        }

        fn enable(clock: u32) -> Result<(), Exception> {
            let generator = Clock::from_binding(clock).ok_or(Exception::UnknownClock { clock })?;
            enable(generator);
            Ok(())
        }

        fn disable(clock: u32) -> Result<(), Exception> {
            let generator = Clock::from_binding(clock).ok_or(Exception::UnknownClock { clock })?;
            disable(generator)
        }

        fn rate(clock: u32) -> Result<u32, Exception> {
            let generator = Clock::from_binding(clock).ok_or(Exception::UnknownClock { clock })?;
            Ok(rate(generator))
        }
    }

    clock::clock_v1_export!(Cprman);

    /// How long a generator may take to finish its last cycle once asked to
    /// stop. A cycle of the slowest output (19.2 MHz / 4095) is about 0.2 ms;
    /// the margin is our own choice.
    const STOP_TIMEOUT_NS: u64 = 10_000_000;

    /// Where the manager's first page is mapped (zero until set up).
    static REGISTERS_VA: AtomicU64 = AtomicU64::new(0);

    /// Admits one caller at a time to the registers.
    static TURNSTILE: Turnstile = Turnstile::new();

    fn init() -> &'static libdevice_driver::DriverInit {
        // SAFETY: Kickstart maps every driver's init record at `INIT_VA`; it
        // is read in this component's own `AddressSpace` only.
        unsafe { driver_init(INIT_VA) }
    }

    /// Record this component's `Thread.Return` key for its exports.
    fn record_return_key() {
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            init().return_key,
        )));
    }

    /// The manager's registers.
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

    /// Run `body` on `clock`'s generator, holding a turn. The PWM generator is
    /// the only one served so far.
    fn with_generator<T>(clock: Clock, body: impl FnOnce(&mut PwmGenerator<'_, '_>) -> T) -> T {
        let _turn = TURNSTILE.enter(init());
        // SAFETY: this caller holds the turn.
        let mut registers = unsafe { registers() };
        match clock {
            Clock::Pwm => body(&mut PwmGenerator(&mut registers)),
        }
    }

    /// A waiter that gives up [`STOP_TIMEOUT_NS`] after its first call.
    fn deadline() -> impl FnMut() -> bool {
        let mut until = None;
        move || {
            let now = libuser::clock::now_ns();
            let until = *until.get_or_insert(now + STOP_TIMEOUT_NS);
            core::hint::spin_loop();
            now < until
        }
    }

    /// `clock_v1.set_rate`: program `setting`, keeping the generator running if
    /// it was.
    fn set_rate(clock: Clock, setting: Setting) -> Result<(), Exception> {
        with_generator(clock, |generator| {
            let running = generator.control().enable();
            program(generator, setting, running, deadline())
                .map_err(|_still_busy| Exception::Busy)?;
            semi::println!(
                "cprman: {clock:?}: {:?} / {} = {} Hz{}, control {:?}",
                setting.source,
                setting.divisor,
                setting.hz(),
                if running { ", running" } else { "" },
                generator.control()
            );
            Ok(())
        })
    }

    /// `clock_v1.enable`.
    fn enable(clock: Clock) {
        with_generator(clock, |generator| {
            let current = generator.control();
            generator.set_control(starting(current));
            semi::println!(
                "cprman: {clock:?} started: {:?}, {:?}",
                generator.control(),
                generator.divisor()
            );
        });
    }

    /// `clock_v1.disable`.
    fn disable(clock: Clock) -> Result<(), Exception> {
        with_generator(clock, |generator| {
            stop(generator, deadline()).map_err(|_still_busy| Exception::Busy)
        })
    }

    /// `clock_v1.rate`.
    fn rate(clock: Clock) -> u32 {
        with_generator(clock, |generator| generator.rate())
    }

    /// Map the manager's registers, set up the turnstile, then park:
    /// everything else happens in calls.
    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let slot = map_device_pages(init)
            .unwrap_or_else(|error| panic!("cprman: mapping registers failed: {:?}", error.code()));
        REGISTERS_VA.store(
            device_address(init, init.regions()[0].start).expect("cprman: registers mapped"),
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
            .unwrap_or_else(|error| panic!("cprman: Retype failed: {:?}", error.code()));
        TURNSTILE.open(turns);
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| panic!("cprman: ready advance failed: {:?}", error.code()));
        let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
        loop {
            let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
        }
    }
}
