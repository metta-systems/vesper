#![no_std]
#![no_main]

//! every-test's exporter of `echo_v1`: the manifest loads this program three
//! times, as `echo_a`, `echo_b` and `echo_c`, each in an `AddressSpace` of its
//! own, so each counts its own calls. A call is refused unless its reference
//! lies in the data area the caller shares with this instance: each instance
//! of an import `via every` gets an area of its own.

use {
    core::sync::atomic::{AtomicU32, Ordering},
    echo::echo_v1::{EchoV1, Exception},
    libdevice_driver::{INIT_VA, driver_init},
    libmeddle_runtime::{DataArea, IoRec},
    libobject::{NotificationKey, RawKey, export::init_return_key, thread::ThreadReturnKey},
};

libuser::entry!(main);

/// The calls this instance has served.
static CALLS: AtomicU32 = AtomicU32::new(0);

fn init() -> &'static libdevice_driver::DriverInit {
    // SAFETY: Kickstart maps every component's init record at `INIT_VA`.
    unsafe { driver_init(INIT_VA) }
}

/// One instance of `echo_v1`.
struct Echo;

impl EchoV1 for Echo {
    fn enter(_badge: u64) {
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            init().return_key,
        )));
    }

    fn count(badge: u64, note: IoRec) -> Result<u32, Exception> {
        let (_client, exported) = init()
            .area_for_badge(badge)
            .ok_or(Exception::NotPermitted)?;
        let area = DataArea {
            base: exported.base,
            len: exported.len,
        };
        if !area.contains(&note) {
            return Err(Exception::NotPermitted);
        }
        Ok(CALLS.fetch_add(1, Ordering::AcqRel) + 1)
    }
}

echo::echo_v1_export!(Echo);

/// Nothing to set up: park, and serve calls.
fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}
