#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! driver-test's probe driver (see `driver-protocol`). It checks what its
//! init page hands it: it maps its register pages, reads an identifying
//! register, acks each interrupt, carves an object from its own memory, and
//! then calls `echo_v1` through its imports and reports on the key
//! driver-test installed. It also exports `echo_v1` to the probe instances
//! whose nodes link to its node. Any failure panics, which
//! ends the QEMU run with a failure.
//!
//! The PL011 instance also checks the root dispatcher's priorities with its
//! further Threads (`driver_threads!`): two normal Threads spin, and a
//! high-priority Thread sleeps briefly again and again; each time it wakes the
//! dispatcher must run it at once, ahead of the spinners, rather than after a
//! spinner's 10 ms slice. Its first Thread reports only once that passed.

use {
    core::{
        ptr::read_volatile,
        sync::atomic::{AtomicBool, AtomicU32, Ordering},
    },
    driver_echo::echo_v1::{EchoV1, Exception},
    driver_echo_client::echo_v1::{self, Bindings, Client, Error},
    driver_protocol::{AUX, AUX_UART, PL011, report_key},
    libdevice_driver::{
        DriverInit, INIT_VA, Priority, device_address, driver_compatible, driver_init,
        driver_threads, map_device_pages,
    },
    libobject::{
        EventCountKey, FrameKey, IrqHandlerKey, KeyTableKey, NotificationKey, ObjectType, RawKey,
        Rights, UntypedKey, export::init_return_key, thread::ThreadReturnKey,
    },
    libuser::semihosting as semi,
};

libuser::entry!(main);

driver_compatible!("arm,pl011", "brcm,bcm2835-aux", "brcm,bcm2835-aux-uart");

/// PL011 `UARTPeriphID0`: 0x11 on every PL011.
const PL011_PERIPH_ID0: u64 = 0xfe0;

/// The probe's `echo_v1`, called on the calling probe's Thread inside this
/// probe's `AddressSpace`.
struct Echo;

impl Echo {
    fn init() -> &'static libdevice_driver::DriverInit {
        // SAFETY: Kickstart maps every driver's init record at `INIT_VA`, and
        // an export runs in this driver's own `AddressSpace`.
        unsafe { driver_init(INIT_VA) }
    }
}

impl EchoV1 for Echo {
    fn enter(_badge: u64) {
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            Self::init().return_key,
        )));
    }

    fn identify() -> u64 {
        Self::init().compatible_index
    }

    fn twice(value: u64) -> Result<u64, Exception> {
        value.checked_mul(2).ok_or(Exception::Overflow { value })
    }

    fn frame_base(page: &FrameKey) -> Result<u64, Exception> {
        // The borrowed key is usable only during this call.
        page.get_extent()
            .map(|(base, _size)| base)
            .map_err(|_error| Exception::BadFrame)
    }
}

driver_echo::echo_v1_export!(Echo);

driver_threads!(urgent => Priority::HIGH, spinner, spinner);

/// The count of the probe's `EventCount` the urgent Thread advances it to
/// once the priority check passed.
const PRIORITY_CHECKED: u64 = 1;

/// Spinners spinning, and whether the check is over.
static SPINNING: AtomicU32 = AtomicU32::new(0);
static CHECKED: AtomicBool = AtomicBool::new(false);

/// Sleeps of the urgent Thread, and how long each is.
const SLEEPS: u32 = 30;
const SLEEP_NS: u64 = 2_000_000;

/// How late the urgent Thread may wake: half the root dispatcher's 10 ms
/// slice. Without priorities a spinner's slice often comes first, about
/// 10 ms.
const MAX_LATE_NS: u64 = 5_000_000;

/// Block for good on the probe's `EventCount`, which never reaches the target.
fn park(init: &DriverInit) -> ! {
    let parked = EventCountKey::from_key(RawKey::from_wire(init.event_count));
    loop {
        let _never = parked.await_ge(u64::MAX, EventCountKey::WAIT_INFINITE);
    }
}

/// A normal Thread of the PL011 instance that is always runnable until the
/// priority check is over.
fn spinner(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    if init.compatible_index == PL011 {
        SPINNING.fetch_add(1, Ordering::AcqRel);
        while !CHECKED.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
    }
    park(init)
}

/// The high-priority Thread of the PL011 instance: once both spinners spin,
/// sleep [`SLEEPS`] times and check it woke within [`MAX_LATE_NS`] each time.
fn urgent(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    if init.compatible_index != PL011 {
        park(init);
    }
    let timer = EventCountKey::from_key(RawKey::from_wire(init.event_count));
    let sleep = || {
        timer
            .sleep(SLEEP_NS)
            .unwrap_or_else(|error| panic!("urgent: sleep failed: {:?}", error.code()));
    };
    while SPINNING.load(Ordering::Acquire) < 2 {
        sleep();
    }
    let mut latest_ns = 0;
    for _ in 0..SLEEPS {
        let asleep = libuser::clock::now_ns();
        sleep();
        let late_ns = libuser::clock::now_ns().saturating_sub(asleep + SLEEP_NS);
        latest_ns = latest_ns.max(late_ns);
    }
    CHECKED.store(true, Ordering::Release);
    semi::println!(
        "driver-probe: the high-priority Thread woke at most {} us late beside two spinning Threads",
        latest_ns / 1000
    );
    assert!(
        latest_ns <= MAX_LATE_NS,
        "the high-priority Thread woke {latest_ns} ns late: the spinners ran first"
    );
    timer
        .advance(PRIORITY_CHECKED)
        .unwrap_or_else(|error| panic!("urgent: report failed: {:?}", error.code()));
    park(init)
}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    assert!(!init.regions().is_empty(), "no register ranges");
    assert!(!init.pages().is_empty(), "no register pages");

    let next_slot = map_device_pages(init)
        .unwrap_or_else(|error| panic!("mapping registers failed: {:?}", error.code()));
    let base = device_address(init, init.regions()[0].start).expect("registers mapped");
    // SAFETY: `map_device_pages` mapped the node's register pages read/write
    // as device memory; the register is a 32-bit read without side effects.
    let first_word = unsafe {
        read_volatile(
            (base
                + if init.compatible_index == PL011 {
                    PL011_PERIPH_ID0
                } else {
                    0
                }) as *const u32,
        )
    };
    if init.compatible_index == PL011 {
        assert_eq!(first_word & 0xff, 0x11, "not a PL011");
    }

    for interrupt in init.interrupts() {
        IrqHandlerKey::from_key(RawKey::from_wire(interrupt.handler))
            .ack()
            .unwrap_or_else(|error| {
                panic!("Ack of line {} failed: {:?}", interrupt.line, error.code())
            });
    }

    let own_table = KeyTableKey::from_key(RawKey::from_wire(init.table));
    UntypedKey::from_key(RawKey::from_wire(init.memory))
        .retype(
            ObjectType::NOTIFICATION,
            0,
            0,
            1,
            &own_table,
            u32::try_from(next_slot).unwrap_or(u32::MAX),
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("Retype from own memory failed: {:?}", error.code()));

    // `uart1` links to `aux` through `clocks`; the other nodes' first clock is
    // the clock manager, which no probe serves.
    let echo = Client::new(Bindings::from_imports(init.imports()));
    if init.compatible_index == AUX_UART {
        assert_eq!(echo.identify(), Ok(AUX), "uart1 calls the aux instance");
        assert_eq!(echo.twice(21), Ok(42));
        assert_eq!(
            echo.twice(u64::MAX),
            Err(Error::Exception(echo_v1::Exception::Overflow {
                value: u64::MAX
            }))
        );

        // Lend Frames carved from the probe's own memory through
        // `frame_base`: the exporter reads the extent through the borrowed
        // key. Its placement window is one slot, so the second call works
        // only because the first call's borrowed entry was removed.
        let mut lend_slot = u32::try_from(next_slot + 1).unwrap_or(u32::MAX);
        let mut carve_frame = |rights: Rights| {
            let frame = UntypedKey::from_key(RawKey::from_wire(init.memory))
                .retype(ObjectType::FRAME, 12, 0, 1, &own_table, lend_slot, rights)
                .unwrap_or_else(|error| panic!("Frame Retype failed: {:?}", error.code()));
            lend_slot += 1;
            FrameKey::from_key(frame)
        };
        let lendable = carve_frame(Rights(Rights::READ | Rights::GRANT | Rights::LEND));
        let (own_base, _size) = lendable
            .get_extent()
            .unwrap_or_else(|error| panic!("GetExtent failed: {:?}", error.code()));
        assert_eq!(echo.frame_base(&lendable), Ok(own_base));
        assert_eq!(echo.frame_base(&lendable), Ok(own_base));
        // Without `LEND` the kernel refuses the call before it starts
        // (`InsufficientRights`, status 6); without `GRANT` the exporter
        // cannot read the extent and answers with its exception.
        assert_eq!(
            echo.frame_base(&carve_frame(Rights(Rights::READ | Rights::GRANT))),
            Err(Error::Call {
                status: 6,
                detail: [0, 0]
            })
        );
        assert_eq!(
            echo.frame_base(&carve_frame(Rights(Rights::READ | Rights::LEND))),
            Err(Error::Exception(echo_v1::Exception::BadFrame))
        );
        // A raised exception and a refused call both left the window free.
        assert_eq!(echo.frame_base(&lendable), Ok(own_base));
    } else {
        assert_eq!(echo.identify(), Err(Error::NotGranted));
    }

    // The priority check runs beside these checks; report after it passed.
    if init.compatible_index == PL011 {
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .await_ge(PRIORITY_CHECKED, EventCountKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("priority check wait failed: {:?}", error.code()));
    }

    semi::println!(
        "driver-probe: compatible {}, {} pages, {} interrupts, {} imports, first register {first_word:#x}",
        init.compatible_index,
        init.pages().len(),
        init.interrupts().len(),
        init.imports().len()
    );
    NotificationKey::from_key(report_key(init.table))
        .signal(0)
        .unwrap_or_else(|error| panic!("report failed: {:?}", error.code()));
    let interrupts = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = interrupts.await_bits(NotificationKey::WAIT_INFINITE);
    }
}
