// Host builds only check the interface implementation; the component itself
// runs on Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(all(target_os = "vesper", feature = "qemu"), feature(format_args_nl))]

//! The mailbox component: serves the `VideoCore` property mailbox
//! (`brcm,bcm2835-mbox`) to other components through `mailbox_v1` (see
//! `interfaces/mailbox_v1.if`).
//!
//! Kickstart loads it as a driver: on the Raspberry Pi 3 its register page is
//! the BCM interrupt controller's, handed to it as a device Frame. Its
//! Thread maps the registers and waits for the mailbox interrupt. Requests
//! are not served yet: every function answers `firmware_error` with code
//! [`NOT_SERVED`].

use mailbox::mailbox_v1::{__runtime::IoRec, Clock, Device, Exception, MailboxV1};

/// `firmware_error` code of every answer until requests are served.
const NOT_SERVED: u32 = u32::MAX;

/// The firmware mailbox.
struct VideoCoreMailbox;

impl VideoCoreMailbox {
    const fn not_served<T>() -> Result<T, Exception> {
        Err(Exception::FirmwareError { code: NOT_SERVED })
    }
}

impl MailboxV1 for VideoCoreMailbox {
    #[cfg(target_os = "vesper")]
    fn enter() {
        component::record_return_key();
    }

    fn get_board_revision() -> Result<u32, Exception> {
        Self::not_served()
    }

    fn get_board_serial() -> Result<u64, Exception> {
        Self::not_served()
    }

    fn get_clock_rate(_which: Clock) -> Result<u32, Exception> {
        Self::not_served()
    }

    fn get_power_state(_which: Device) -> Result<bool, Exception> {
        Self::not_served()
    }

    fn set_power_state(_which: Device, _on: bool) -> Result<bool, Exception> {
        Self::not_served()
    }

    fn property(_tags: IoRec) -> Result<(), Exception> {
        Self::not_served()
    }
}

mailbox::mailbox_v1_export!(VideoCoreMailbox);

#[cfg(target_os = "vesper")]
mod component {
    use {
        libdevice_driver::{INIT_VA, driver_compatible, driver_init, map_device_pages},
        libobject::{NotificationKey, RawKey, export::init_return_key, thread::ThreadReturnKey},
    };

    libuser::entry!(main);

    driver_compatible!("brcm,bcm2835-mbox");

    /// Offset of mailbox 0's status register in the node's registers.
    #[cfg(feature = "qemu")]
    const MAILBOX0_STATUS: u64 = 0x18;

    /// Record this component's `Thread.Return` key for its exports.
    pub fn record_return_key() {
        // SAFETY: Kickstart maps every driver's init record at `INIT_VA`, and
        // an export runs in this component's own `AddressSpace`.
        let init = unsafe { driver_init(INIT_VA) };
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            init.return_key,
        )));
    }

    /// Under QEMU, read the mapped mailbox status register and report it.
    #[cfg(feature = "qemu")]
    fn report_registers(init: &libdevice_driver::DriverInit) {
        use {
            core::ptr::read_volatile, libdevice_driver::device_address,
            libuser::semihosting as semi,
        };

        let registers =
            device_address(init, init.regions()[0].start).expect("mailbox: registers mapped");
        // SAFETY: `map_device_pages` mapped the node's registers as device
        // memory; reading the status register has no side effects.
        let status = unsafe { read_volatile((registers + MAILBOX0_STATUS) as *const u32) };
        semi::println!(
            "mailbox: registers at {:#x}, status {status:#x}, {} interrupts",
            init.regions()[0].start,
            init.interrupts().len()
        );
    }

    #[cfg(not(feature = "qemu"))]
    fn report_registers(_init: &libdevice_driver::DriverInit) {}

    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        map_device_pages(init).unwrap_or_else(|error| {
            panic!("mailbox: mapping registers failed: {:?}", error.code())
        });
        report_registers(init);
        let interrupts = NotificationKey::from_key(RawKey::from_wire(init.notification));
        loop {
            let _bits = interrupts.wait(NotificationKey::WAIT_INFINITE);
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}
