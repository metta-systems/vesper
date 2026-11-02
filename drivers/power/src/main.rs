// Host builds only check the interface implementation; the component itself
// runs on Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! The power component: the Raspberry Pi's power control behind `power_v1`
//! (see `interfaces/power_v1.if`).
//!
//! Kickstart loads it for the power management node (`brcm,bcm2835-pm`), whose
//! registers halt and reset the board, and wires its import of `mailbox_v1`
//! through the firmware node (see `interfaces/imports.if`): powering the
//! firmware's devices off is a mailbox request per device.
//!
//! `halt` powers the devices off, marks the halt for the firmware in the reset
//! status register and resets through the watchdog, so the firmware stops
//! instead of booting again.

use power::power_v1::{Exception, PowerV1};

/// `firmware_error` code when the mailbox cannot be reached or answers
/// something unexpected.
const MAILBOX_FAILED: u32 = u32::MAX;

/// The board.
struct Board;

impl PowerV1 for Board {
    #[cfg(target_os = "vesper")]
    fn enter(_badge: u64) {
        component::record_return_key();
    }

    fn devices_off() -> Result<(), Exception> {
        devices_off()
    }

    fn halt() -> Result<(), Exception> {
        halt().map(|never| match never {})
    }

    fn reset() {
        reset();
    }
}

power::power_v1_export!(Board);

#[cfg(target_os = "vesper")]
use component::{devices_off, halt, reset};

#[cfg(not(target_os = "vesper"))]
fn devices_off() -> Result<(), Exception> {
    Err(Exception::FirmwareError {
        code: MAILBOX_FAILED,
    })
}

#[cfg(not(target_os = "vesper"))]
fn halt() -> Result<core::convert::Infallible, Exception> {
    Err(Exception::FirmwareError {
        code: MAILBOX_FAILED,
    })
}

#[cfg(not(target_os = "vesper"))]
fn reset() {}

#[cfg(target_os = "vesper")]
mod component {
    use {
        super::{Exception, MAILBOX_FAILED},
        core::{
            convert::Infallible,
            sync::atomic::{AtomicU64, Ordering},
        },
        libdevice_driver::{
            INIT_VA, Turnstile, device_address, driver_compatible, driver_init, map_device_pages,
        },
        libobject::{
            EventCountKey, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
            export::init_return_key, thread::ThreadReturnKey,
        },
        mailbox_client::mailbox_v1::{Bindings, Client, Device, Error},
        power_impl::{
            MmioRegisterBlock, RegisterBlock, full_reset_control, halt_status, watchdog_start,
        },
    };

    libuser::entry!(main);

    driver_compatible!("brcm,bcm2835-pm");

    /// The devices the firmware powers, in the order of its ids.
    const DEVICES: [Device; 9] = [
        Device::SdCard,
        Device::Uart0,
        Device::Uart1,
        Device::UsbHcd,
        Device::I2c0,
        Device::I2c1,
        Device::I2c2,
        Device::Spi,
        Device::Ccp2tx,
    ];

    /// Where the first register range of the block is mapped (zero until set up).
    static REGISTERS_VA: AtomicU64 = AtomicU64::new(0);

    /// Admits one caller at a time: the mailbox stack it calls on is single-user.
    static TURNSTILE: Turnstile = Turnstile::new();

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

    /// The block's reset registers. Only `reset` and `halt` use them, and
    /// both end the board's run.
    fn registers() -> MmioRegisterBlock<'static> {
        // SAFETY: `main` mapped the registers for good before the first call.
        unsafe {
            RegisterBlock::new_mmio(REGISTERS_VA.load(Ordering::Acquire) as *mut RegisterBlock)
        }
    }

    /// The firmware's response code behind a failed mailbox call.
    fn firmware_code(error: &Error) -> u32 {
        match error {
            Error::Exception(mailbox_client::mailbox_v1::Exception::FirmwareError { code }) => {
                *code
            }
            _ => MAILBOX_FAILED,
        }
    }

    /// Power every device the firmware controls off, one request each.
    pub fn devices_off() -> Result<(), Exception> {
        let init = init();
        let _turn = TURNSTILE.enter(init);
        let mailbox = Client::new(Bindings::from_imports(init.imports()));
        for device in DEVICES {
            mailbox
                .set_power_state(device, false)
                .map_err(|error| Exception::FirmwareError {
                    code: firmware_code(&error),
                })?;
        }
        Ok(())
    }

    /// Reset the board through the watchdog; the board stops running here.
    pub fn reset() -> ! {
        let mut registers = registers();
        registers.write_watchdog(watchdog_start());
        registers.modify_reset_control(full_reset_control);
        loop {
            core::hint::spin_loop();
        }
    }

    /// Power the devices off, mark the halt and reset.
    pub fn halt() -> Result<Infallible, Exception> {
        devices_off()?;
        let mut registers = registers();
        registers.modify_reset_status(halt_status);
        reset()
    }

    /// Map the block's registers, set up the turnstile, then park: everything
    /// else happens in calls.
    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let slot = map_device_pages(init)
            .unwrap_or_else(|error| panic!("power: mapping registers failed: {:?}", error.code()));
        REGISTERS_VA.store(
            device_address(init, init.regions()[0].start).expect("power: registers mapped"),
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
            .unwrap_or_else(|error| panic!("power: Retype failed: {:?}", error.code()));
        TURNSTILE.open(turns);
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| panic!("power: ready advance failed: {:?}", error.code()));
        let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
        loop {
            let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}
