#![no_std]
#![no_main]

//! board-test's driver of the function behind the PCIe root port: the
//! Raspberry Pi 4's VL805 (`usb@1,0`) and the Raspberry Pi 5's RP1 (`rp1`),
//! nodes its manifest entry names by path.
//!
//! It claims the function from the host bridge driver through `pci_v1`,
//! lending a table of its own for the BAR Frames, and logs what the claimed
//! record says: the function's IDs, its MSI bit and its BARs. It maps the
//! first Frame and logs the BAR's first word (the VL805's `CAPLENGTH` and
//! `HCIVERSION`, or the start of RP1's peripheral window). Then it asks the
//! bridge driver to raise its MSI from software and waits for the bit on its
//! own Notification. All of it goes to the log server as `pci-probe:` lines.

use {
    libdevice_driver::{DriverInit, device_page_va, driver_init},
    liblog_client::log_println,
    libobject::{
        CapError, FrameAttribute, FrameKey, KeySlot, KeyTableKey, NotificationKey, ObjectType,
        RawKey, Rights, UntypedKey,
    },
    pci::claimed,
    pci_client::pci_v1::{Bindings, Client},
};

libuser::entry!(main);

/// Size and guard of the table the BAR Frames go into.
const FRAME_TABLE_BITS: u8 = 6;
const FRAME_TABLE_GUARD: u32 = 0x0_BA25;

/// How long the probe waits for its MSI.
const MSI_WAIT_NS: u64 = 100_000_000;

/// The key of slot `index` of the Frame table.
fn frame_key(index: u32) -> RawKey {
    RawKey::new(KeySlot((FRAME_TABLE_GUARD << FRAME_TABLE_BITS) | index), 1)
}

/// The BAR entries of a claimed record: `[number, bus address, CPU address,
/// size]` each.
type Bars = [[u64; claimed::BAR_WORDS]; claimed::MAX_BARS];

/// The claimed record's words: `(identity, class, msi_bits, count, bars)`.
fn read_claimed(pci: &Client) -> Option<(u64, u64, u64, usize, Bars)> {
    let area = pci.data_area()?;
    let mut header = [0; 4];
    if !area.read_u64s(claimed::IDENTITY, &mut header) {
        return None;
    }
    let count = usize::try_from(header[3]).ok()?.min(claimed::MAX_BARS);
    let mut bars = [[0; claimed::BAR_WORDS]; claimed::MAX_BARS];
    for (index, bar) in bars.iter_mut().take(count).enumerate() {
        let offset = claimed::BARS + (index * claimed::BAR_WORDS * 8) as u64;
        if !area.read_u64s(offset, bar) {
            return None;
        }
    }
    Some((header[0], header[1], header[2], count, bars))
}

/// Map the first Frame of the claim at the start of the device window and read
/// its first word.
fn first_word(
    init: &DriverInit,
    own: &KeyTableKey,
    frames: &KeyTableKey,
    slot: u32,
) -> Option<u32> {
    let copy = frames
        .copy_derive(
            frame_key(0),
            own,
            slot,
            Rights(Rights::READ | Rights::WRITE),
        )
        .map_err(|error| log_println!("pci-probe: Frame copy failed: {:?}", error.code()))
        .ok()?;
    let va = device_page_va(0);
    FrameKey::from_key(copy)
        .map(
            RawKey::from_wire(init.address_space),
            va,
            Rights(Rights::READ | Rights::WRITE),
            FrameAttribute::DeviceStrict,
        )
        .map_err(|error| log_println!("pci-probe: Frame.Map failed: {:?}", error.code()))
        .ok()?;
    // SAFETY: the Frame is mapped read/write as device memory at `va` for
    // the component's lifetime, and only this Thread touches it.
    Some(unsafe { (va as *const u32).read_volatile() })
}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    let pci = Client::new(Bindings::from_imports(init.imports()));
    let own = KeyTableKey::from_key(RawKey::from_wire(init.table));
    let first_free = u32::try_from(init.first_free_slot).unwrap_or(u32::MAX);
    let frames = UntypedKey::from_key(RawKey::from_wire(init.memory))
        .retype(
            ObjectType::KEY_TABLE,
            FRAME_TABLE_BITS,
            FRAME_TABLE_GUARD,
            1,
            &own,
            first_free,
            Rights::all(),
        )
        .map_or_else(
            |error| panic!("pci-probe: KeyTable Retype failed: {:?}", error.code()),
            KeyTableKey::from_key,
        );

    let found = pci
        .data_area()
        .and_then(|area| area.reference(0, claimed::BYTES))
        .expect("pci-probe: a data area for the record");
    let msi_bits = match pci.claim(&frames, found, 0) {
        Ok(installed) => match read_claimed(&pci) {
            Some((identity, class, msi_bits, count, bars)) => {
                log_println!(
                    "pci-probe: claimed {:04x}:{:04x} class {class:#08x}, {installed} Frames, MSI bits {msi_bits:#x}",
                    identity & 0xffff,
                    identity >> 16 & 0xffff
                );
                for [number, bus, cpu, size] in bars.iter().take(count) {
                    log_println!("pci-probe: BAR{number} {bus:#x}+{size:#x}, CPU {cpu:#x}");
                }
                if installed > 0
                    && let Some(word) = first_word(init, &own, &frames, first_free + 1)
                {
                    log_println!("pci-probe: BAR{} word 0: {word:#010x}", bars[0][0]);
                }
                msi_bits
            }
            None => {
                log_println!("pci-probe: claimed, but the record is unreadable");
                0
            }
        },
        Err(error) => {
            log_println!("pci-probe: claim failed: {error:?}");
            0
        }
    };

    let notification = NotificationKey::from_key(RawKey::from_wire(init.notification));
    match pci.raise_msi() {
        Ok(()) => {
            let arrived = match notification.await_bits(MSI_WAIT_NS) {
                Ok(bits) => bits & msi_bits != 0,
                Err(CapError::TimedOut { .. }) => false,
                Err(error) => panic!("pci-probe: MSI wait failed: {:?}", error.code()),
            };
            if arrived {
                log_println!("pci-probe: MSI raised in software arrived on bits {msi_bits:#x}");
            } else {
                log_println!("pci-probe: MSI raised in software did not arrive");
            }
        }
        Err(error) => log_println!("pci-probe: raise_msi failed: {error:?}"),
    }
    loop {
        if let Err(error) = notification.await_bits(NotificationKey::WAIT_INFINITE) {
            panic!("pci-probe: idle wait failed: {:?}", error.code());
        }
    }
}
