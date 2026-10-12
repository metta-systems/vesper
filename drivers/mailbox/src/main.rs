#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! The mailbox component: serves the `VideoCore` property mailbox to other
//! components through `mailbox_v1` (see `interfaces/mailbox_v1.if`).
//!
//! This is the stub the generated crates are built against: it exports
//! every function, and every function answers `firmware_error` with code
//! [`NOT_SERVED`] because no mailbox registers are reached yet.

use mailbox::mailbox_v1::{__runtime::IoRec, Clock, Device, Exception, MailboxV1};

/// `firmware_error` code of every stub answer.
const NOT_SERVED: u32 = u32::MAX;

/// The firmware mailbox.
struct VideoCoreMailbox;

impl VideoCoreMailbox {
    const fn not_served<T>() -> Result<T, Exception> {
        Err(Exception::FirmwareError { code: NOT_SERVED })
    }
}

impl MailboxV1 for VideoCoreMailbox {
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
libuser::passive!();

#[cfg(not(target_os = "vesper"))]
fn main() {}
