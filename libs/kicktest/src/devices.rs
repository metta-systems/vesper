//! Checks on the device Untypeds Kickstart hands out.

use {
    kickstart::bootstrap::{device_untypeds, kernel_owned_device_ranges, video_untypeds},
    libcomponent_loader::keys::{BOOT_TABLE_GUARD, boot_key},
    libdevicetree::{find_compatible, regions},
    libobject::RawKey,
    nucleus::{api::key_entry::KeyEntry, objects::KeyTable},
};

const PAGE: u64 = 0x1000;

/// Where the device tree puts the ARM local interrupt block, which also holds
/// the core timer controls.
fn local_interrupt_block() -> u64 {
    let tree = kickstart::device_tree::at_el1().index();
    find_compatible(tree, &["brcm,bcm2836-l1-intc"])
        .and_then(|(node, _)| regions(&node).next())
        .expect("the device tree has no ARM local interrupt block")
        .start
}

/// Every device Untyped in the boot table, as `(key, start, end)`.
pub fn device_untyped_extents(keytable_addr: u64) -> impl Iterator<Item = (RawKey, u64, u64)> {
    let range = device_untypeds();
    (range.first_slot..range.first_slot + range.count).map(move |slot| {
        let key = boot_key(slot, 1);
        // SAFETY: the boot table is the live carved boot KeyTable; this
        // read-only borrow ends within the closure.
        let table = unsafe { &*KeyTable::at_carve(keytable_addr) };
        let region = *table
            .lookup(key, BOOT_TABLE_GUARD)
            .and_then(KeyEntry::as_untyped)
            .unwrap_or_else(|error| panic!("device Untyped {slot}: {:?}", error.code()));
        assert!(region.is_device, "slot {slot} is not device memory");
        (key, region.paddr, region.paddr + (1 << region.size_bits))
    })
}

/// The first boot-table key of the `VideoCore` memory Untypeds (Kickstart sets
/// their Frames' attribute ceiling to write-combining), if the board has any.
pub fn first_video_untyped() -> Option<RawKey> {
    let range = video_untypeds();
    (range.count > 0).then(|| boot_key(range.first_slot, 1))
}

/// Whether `key` names one of the `VideoCore` memory Untypeds.
pub fn is_video_untyped(key: RawKey) -> bool {
    let range = video_untypeds();
    (range.first_slot..range.first_slot + range.count).any(|slot| boot_key(slot, 1) == key)
}

/// No device Untyped covers a page of a kernel-owned range or of the ARM
/// local interrupt block, and none overlap each other.
pub fn assert_device_untypeds_avoid_the_kernel(keytable_addr: u64) {
    let range = device_untypeds();
    assert!(range.count > 0, "Kickstart handed out no device Untypeds");
    let local_start = local_interrupt_block();
    let local_end = local_start + PAGE;
    let mut previous_end = 0;
    for (_, start, end) in device_untyped_extents(keytable_addr) {
        assert!(
            start >= previous_end,
            "device Untypeds overlap at {start:#x}"
        );
        previous_end = end;
        assert!(
            end <= local_start || start >= local_end,
            "device Untyped {start:#x}..{end:#x} covers the local interrupt block"
        );
        for &(owned, size) in kernel_owned_device_ranges() {
            let (owned_start, owned_end) =
                (owned & !(PAGE - 1), (owned + size).next_multiple_of(PAGE));
            assert!(
                end <= owned_start || start >= owned_end,
                "device Untyped {start:#x}..{end:#x} overlaps kernel-owned {owned:#x}"
            );
        }
    }
}
