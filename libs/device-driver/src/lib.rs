#![no_std]

//! What Kickstart and an EL0 device driver agree on.
//!
//! A driver declares the device tree `compatible` strings it serves with
//! [`driver_compatible!`], in a link section of its own binary. For each
//! enabled node that matches, Kickstart loads one instance of the driver in
//! its own `AddressSpace` and hands it everything through an init page: a
//! [`DriverInit`] record at [`INIT_VA`], whose address the driver's Thread
//! receives in `x0` on its first entry.
//!
//! The record names, as keys in the driver's own table:
//!
//! - the node's register pages: page-sized device Untypeds the driver carves
//!   its device Frames from, or, for a page several drivers share, a copy of
//!   one device Frame ([`map_device_pages`] does both);
//! - one `IRQHandler` per interrupt of the node, every one bound to the
//!   driver's Notification, interrupt *i* signalling bit *i*; each handler is
//!   enabled by its first `Ack`;
//! - the Notification, a RAM Untyped, the driver's Time, its `AddressSpace`
//!   and its own table.

use libobject::{CapError, FrameKey, KeyTableKey, ObjectType, RawKey, Rights, UntypedKey};

/// Name of the section holding a driver's NUL-terminated `compatible`
/// strings.
pub const COMPATIBLE_SECTION: &str = ".driver_compatible";

/// Declare the device tree `compatible` strings a driver serves.
#[macro_export]
macro_rules! driver_compatible {
    ($($compatible:literal),+ $(,)?) => {
        const DRIVER_COMPATIBLE_LIST: &str = concat!($($compatible, "\0"),+);

        #[cfg_attr(target_os = "vesper", unsafe(link_section = ".driver_compatible"))]
        #[used]
        static DRIVER_COMPATIBLE: [u8; DRIVER_COMPATIBLE_LIST.len()] =
            *DRIVER_COMPATIBLE_LIST.as_bytes().first_chunk().unwrap();
    };
}

/// Where the init page is mapped, read-only, in every driver.
pub const INIT_VA: u64 = 0x2000_0000;
/// The 2 MiB span holding the driver's guarded stack.
pub const STACK_REGION: u64 = 0x3000_0000;
/// The 2 MiB span where a driver maps its device pages: its L3 table is
/// installed, so only Frames need mapping there.
pub const DEVICE_WINDOW_VA: u64 = 0x3800_0000;

/// Most register ranges of one node.
pub const MAX_REGIONS: usize = 4;
/// Most register pages of one node.
pub const MAX_DEVICE_PAGES: usize = 16;
/// Most interrupts of one node: one Notification bit each.
pub const MAX_INTERRUPTS: usize = 16;

/// One `reg` range of the node, as a CPU physical address range.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Region {
    pub start: u64,
    pub size: u64,
}

/// [`DevicePage::kind`]: a page-sized device Untyped.
pub const PAGE_UNTYPED: u64 = 0;
/// [`DevicePage::kind`]: a copy of a device Frame other drivers also hold.
pub const PAGE_FRAME: u64 = 1;

/// One register page of the node.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DevicePage {
    /// The page's physical address
    pub paddr: u64,
    /// Wire key in the driver's table
    pub key: u64,
    /// [`PAGE_UNTYPED`] or [`PAGE_FRAME`]
    pub kind: u64,
}

/// One interrupt of the node.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Interrupt {
    /// Wire key of its `IRQHandler` in the driver's table
    pub handler: u64,
    /// The Notification bits it signals
    pub bits: u64,
    /// The interrupt line
    pub line: u64,
}

/// The init page's record.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DriverInit {
    /// Which of the driver's `compatible` strings the node matched, by its
    /// position in [`driver_compatible!`]
    pub compatible_index: u64,
    /// The driver's own `AddressSpace` (`MAP`)
    pub address_space: u64,
    /// The driver's own table (`INSTALL`), for Retype destinations
    pub table: u64,
    /// First slot of the driver's table nothing occupies
    pub first_free_slot: u64,
    /// The Notification the node's interrupts signal (`RECV`)
    pub notification: u64,
    /// A RAM Untyped for the driver's own objects
    pub memory: u64,
    /// Bytes `memory` covers
    pub memory_size: u64,
    /// The Time the driver's Thread runs on
    pub time: u64,
    pub region_count: u64,
    pub page_count: u64,
    pub interrupt_count: u64,
    pub regions: [Region; MAX_REGIONS],
    pub pages: [DevicePage; MAX_DEVICE_PAGES],
    pub interrupts: [Interrupt; MAX_INTERRUPTS],
}

const _: () = assert!(size_of::<DriverInit>() <= 4096);

impl DriverInit {
    /// A record with nothing in it.
    pub const EMPTY: Self = Self {
        compatible_index: 0,
        address_space: 0,
        table: 0,
        first_free_slot: 0,
        notification: 0,
        memory: 0,
        memory_size: 0,
        time: 0,
        region_count: 0,
        page_count: 0,
        interrupt_count: 0,
        regions: [Region { start: 0, size: 0 }; MAX_REGIONS],
        pages: [DevicePage {
            paddr: 0,
            key: 0,
            kind: 0,
        }; MAX_DEVICE_PAGES],
        interrupts: [Interrupt {
            handler: 0,
            bits: 0,
            line: 0,
        }; MAX_INTERRUPTS],
    };

    /// The node's register ranges.
    pub fn regions(&self) -> &[Region] {
        &self.regions[..clamp(self.region_count, MAX_REGIONS)]
    }

    /// The node's register pages, by address.
    pub fn pages(&self) -> &[DevicePage] {
        &self.pages[..clamp(self.page_count, MAX_DEVICE_PAGES)]
    }

    /// The node's interrupts, in device tree order.
    pub fn interrupts(&self) -> &[Interrupt] {
        &self.interrupts[..clamp(self.interrupt_count, MAX_INTERRUPTS)]
    }
}

fn clamp(count: u64, limit: usize) -> usize {
    usize::try_from(count).map_or(limit, |count| count.min(limit))
}

/// The driver's init record.
///
/// # Safety
///
/// Only in a driver Kickstart loaded, with `address` the value its Thread
/// received in `x0`: Kickstart maps the record there before the Thread runs
/// and never changes it.
pub unsafe fn driver_init(address: u64) -> &'static DriverInit {
    // SAFETY: forwarded caller contract.
    unsafe { &*(address as *const DriverInit) }
}

/// Map every register page of `init` at consecutive pages from
/// [`DEVICE_WINDOW_VA`], read/write and never executable, carving a device
/// Frame from each page Untyped into the driver's table from
/// `init.first_free_slot`. Returns the first free slot afterwards.
///
/// # Errors
///
/// The first failing Retype or `Frame.Map`.
pub fn map_device_pages(init: &DriverInit) -> Result<u64, CapError> {
    let table = KeyTableKey::from_key(RawKey::from_wire(init.table));
    let address_space = RawKey::from_wire(init.address_space);
    let mut slot = init.first_free_slot;
    for (index, page) in init.pages().iter().enumerate() {
        let frame = if page.kind == PAGE_UNTYPED {
            let destination =
                u32::try_from(slot).map_err(|_overflow| CapError::InvalidOperation)?;
            slot += 1;
            UntypedKey::from_key(RawKey::from_wire(page.key)).retype(
                ObjectType::FRAME,
                12,
                0,
                1,
                &table,
                destination,
                Rights(Rights::READ | Rights::WRITE),
            )?
        } else {
            RawKey::from_wire(page.key)
        };
        FrameKey::from_key(frame).map(
            address_space,
            device_page_va(index),
            Rights(Rights::READ | Rights::WRITE),
            0,
        )?;
    }
    Ok(slot)
}

/// Where [`map_device_pages`] maps the page at `index` of `init.pages()`.
pub const fn device_page_va(index: usize) -> u64 {
    DEVICE_WINDOW_VA + (index as u64) * 4096
}

/// The address [`map_device_pages`] makes physical address `paddr` of the
/// node's registers visible at.
pub fn device_address(init: &DriverInit, paddr: u64) -> Option<u64> {
    let page = paddr & !0xfff;
    init.pages()
        .iter()
        .position(|candidate| candidate.paddr == page)
        .map(|index| device_page_va(index) + (paddr & 0xfff))
}
