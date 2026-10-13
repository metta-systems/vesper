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
//! - the Notification, a RAM Untyped (with its physical base and the address
//!   the node's device reaches it at, for DMA buffers), the driver's Time, its
//!   `AddressSpace` and its own table;
//! - an `EventCount` only the driver's own Threads hold, for them to hand
//!   work to each other;
//! - its `Thread.Return` key, for the functions it exports, and one
//!   [`ImportRecord`] per function of another component its
//!   `interfaces/imports.if` uses: the function's Invocation, in the
//!   driver's table, into the instance the driver's node links to with the
//!   `uses ... via` property, and the target stack reserved for the
//!   driver's calls into that instance (one per instance it calls).
//!
//! A driver that needs more Threads than its interrupt Thread declares their
//! entries with [`driver_threads!`]. Kickstart starts each on its own stack
//! and Time with the init page's address in `x0`, alongside the first one;
//! only the first Thread receives the interrupts.
use {
    core::{
        marker::PhantomData,
        sync::atomic::{AtomicU64, Ordering},
    },
    libmeddle_runtime::ImportRecord,
    libobject::{
        CapError, EventCountKey, FrameKey, KeySlot, KeyTableKey, ObjectType, RawKey, Rights,
        UntypedKey,
    },
};

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

/// Name of the section holding the entries of a driver's further Threads.
pub const THREADS_SECTION: &str = ".driver_threads";

/// Most Threads of one driver besides the first.
pub const MAX_FURTHER_THREADS: usize = 4;

/// Declare the entries of the driver's further Threads, each a
/// `fn(u64) -> !` receiving the init page's address like the first Thread's
/// entry.
#[macro_export]
macro_rules! driver_threads {
    ($($entry:path),+ $(,)?) => {
        #[cfg_attr(target_os = "vesper", unsafe(link_section = ".driver_threads"))]
        #[used]
        static DRIVER_THREADS: [extern "C" fn(u64) -> !; [$(stringify!($entry)),+].len()] = [$({
            extern "C" fn thread_entry(init_va: u64) -> ! {
                $entry(init_va)
            }
            thread_entry
        }),+];
        const _: () = assert!(DRIVER_THREADS.len() <= $crate::MAX_FURTHER_THREADS);
    };
}

/// The 2 MiB span where Kickstart maps the data areas shared between clients
/// and exporters (`DriverInit::areas`, `ImportRecord::area_base`): its L3 table
/// is installed in every participant. Each pair's area has one address, the
/// same in both.
pub const DATA_WINDOW_VA: u64 = 0x3a00_0000;

/// Most data areas an exporting driver serves: one per client instance.
pub const MAX_AREAS: usize = 8;

/// Most resources a component maps.
pub const MAX_RESOURCES: usize = 4;

/// Where the init page is mapped, read-only, in every driver.
pub const INIT_VA: u64 = 0x2000_0000;
/// The 2 MiB span holding the driver's guarded stack.
pub const STACK_REGION: u64 = 0x3000_0000;
/// The 2 MiB span where a driver maps its device pages: its L3 table is
/// installed, so only Frames need mapping there.
pub const DEVICE_WINDOW_VA: u64 = 0x3800_0000;

/// Where a component that imports `fb_v1` maps the framebuffer's Frames
/// ([`FbWindow`]): `FB_WINDOW_SPANS` consecutive 2 MiB spans.
pub const FB_WINDOW_VA: u64 = 0x3e00_0000;

/// Spans of 2 MiB in the framebuffer window, and so the spare page tables a
/// component receives: a 1024x768 framebuffer at any alignment touches three.
pub const FB_WINDOW_SPANS: usize = 4;

/// Most register ranges of one node.
pub const MAX_REGIONS: usize = 4;
/// Most register pages of one node.
pub const MAX_DEVICE_PAGES: usize = 16;
/// [`DriverInit::memory_bus`] when the device cannot reach the driver's memory.
pub const NO_BUS_ADDRESS: u64 = u64::MAX;

/// Most `VideoCore` memory blocks one driver receives.
pub const MAX_VIDEO_BLOCKS: usize = 8;

/// Most functions of other components one driver imports.
pub const MAX_IMPORTS: usize = 32;

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

/// One block of `VideoCore` memory: a device Untyped in the driver's table.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VideoBlock {
    /// Wire key of the device Untyped in the driver's table
    pub key: u64,
    /// Physical base of the block
    pub base: u64,
    /// Size of the block as a power of two
    pub size_bits: u64,
}

impl VideoBlock {
    /// Physical address just past the block.
    pub const fn end(&self) -> u64 {
        self.base + (1 << self.size_bits)
    }
}

/// The data area an exporting driver shares with one client instance, and the
/// stack that client's calls run on. A call enters the exporter with its stack
/// pointer inside the stack of the client's own Invocation, which the kernel
/// enforces, so the exporter tells its callers apart by [`calling_area`].
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExportedArea {
    /// Bottom of the target stack reserved for the client
    pub stack_bottom: u64,
    /// End (initial stack pointer) of that stack
    pub stack_end: u64,
    /// Base of the data area, at the same address in the client
    pub base: u64,
    /// Bytes of the data area
    pub len: u64,
}

/// The init page's record.
/// The keys of a client attached to a serial driver's channel (see
/// `libserial-channel`), in the client's table: zero where the client is not
/// attached. The control page and data area are already mapped read/write at
/// `CONTROL_VA` and `DATA_AREA_VA` there.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SerialKeys {
    /// The transmit channel's submit `EventCount`, to advance after submitting
    /// packets (`SEND`)
    pub transmit_submitted: u64,
    /// The transmit channel's completion `EventCount` (`RECV`)
    pub transmit_completed: u64,
    /// The receive channel's completion `EventCount` (`RECV`)
    pub receive_completed: u64,
}

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
    /// The driver's `Thread.Return` key, which every function it exports
    /// records (`libobject::export::init_return_key`) before answering
    pub return_key: u64,
    /// The Notification the node's interrupts signal (`RECV`)
    pub notification: u64,
    /// A RAM Untyped for the driver's own objects
    pub memory: u64,
    /// Physical base of `memory`
    pub memory_paddr: u64,
    /// Where the node's device reaches `memory` through its buses' `dma-ranges`
    /// (the address to give it for DMA), or [`NO_BUS_ADDRESS`]
    pub memory_bus: u64,
    /// Bytes `memory` covers
    pub memory_size: u64,
    /// The Time the driver's first Thread runs on
    pub time: u64,
    /// An `EventCount` only the driver's Threads hold (`SEND | RECV`)
    pub event_count: u64,
    pub region_count: u64,
    pub page_count: u64,
    pub interrupt_count: u64,
    pub regions: [Region; MAX_REGIONS],
    pub pages: [DevicePage; MAX_DEVICE_PAGES],
    pub interrupts: [Interrupt; MAX_INTERRUPTS],
    /// The `VideoCore` memory a driver that asked for it holds: the memory the
    /// firmware allocates framebuffers in, outside every other Untyped
    pub video_count: u64,
    pub video: [VideoBlock; MAX_VIDEO_BLOCKS],
    pub import_count: u64,
    /// Data areas shared with the clients that call this driver's exports
    pub area_count: u64,
    pub areas: [ExportedArea; MAX_AREAS],
    /// The functions of other components the driver imports (its
    /// `interfaces/imports.if`), each linked through its device tree node
    pub imports: [ImportRecord; MAX_IMPORTS],
    /// The keys of the serial channel Kickstart attached the component to, if
    /// its manifest entry says `serial = true`
    pub serial: SerialKeys,
    /// What the component needs to map a framebuffer, if it imports `fb_v1`
    pub fb_window: FbWindow,
    pub resource_count: u64,
    /// The bundled resources mapped read-only in the component, in the order
    /// its manifest entry names them (`resources = [...]`)
    pub resources: [ResourceRecord; MAX_RESOURCES],
}

/// The page tables Kickstart provisions for mapping a framebuffer at
/// [`FB_WINDOW_VA`].
///
/// A 2 MiB Frame maps as a block in the second-level table, a 4 KiB Frame
/// needs a last-level table below it; which spans need which depends on where
/// the firmware puts the framebuffer, so the tables come uninstalled, to be
/// installed (`PageTable.Map` into `l2`) for the spans that hold 4 KiB Frames.
/// All keys are zero when the component imports no `fb_v1`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FbWindow {
    /// The second-level `PageTable` of the component's `AddressSpace`
    pub l2: u64,
    /// Uninstalled last-level `PageTable`s
    pub tables: [u64; FB_WINDOW_SPANS],
}

/// Where one bundled resource is mapped, read-only.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ResourceRecord {
    pub base: u64,
    /// Bytes of the file
    pub len: u64,
}

const _: () = assert!(size_of::<DriverInit>() <= 4096);

impl DriverInit {
    /// A record with nothing in it.
    pub const EMPTY: Self = Self {
        compatible_index: 0,
        address_space: 0,
        table: 0,
        first_free_slot: 0,
        return_key: 0,
        notification: 0,
        memory: 0,
        memory_paddr: 0,
        memory_bus: NO_BUS_ADDRESS,
        memory_size: 0,
        time: 0,
        event_count: 0,
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
        video_count: 0,
        video: [VideoBlock {
            key: 0,
            base: 0,
            size_bits: 0,
        }; MAX_VIDEO_BLOCKS],
        import_count: 0,
        area_count: 0,
        areas: [ExportedArea {
            stack_bottom: 0,
            stack_end: 0,
            base: 0,
            len: 0,
        }; MAX_AREAS],
        imports: [ImportRecord {
            type_code: 0,
            function: 0,
            invocation: 0,
            stack_end: 0,
            area_base: 0,
            area_len: 0,
        }; MAX_IMPORTS],
        serial: SerialKeys {
            transmit_submitted: 0,
            transmit_completed: 0,
            receive_completed: 0,
        },
        fb_window: FbWindow {
            l2: 0,
            tables: [0; FB_WINDOW_SPANS],
        },
        resource_count: 0,
        resources: [ResourceRecord { base: 0, len: 0 }; MAX_RESOURCES],
    };

    /// The node's register ranges.
    pub fn regions(&self) -> &[Region] {
        &self.regions[..clamp(self.region_count, MAX_REGIONS)]
    }

    /// The node's register pages, by address.
    pub fn pages(&self) -> &[DevicePage] {
        &self.pages[..clamp(self.page_count, MAX_DEVICE_PAGES)]
    }

    /// The blocks of `VideoCore` memory the driver holds.
    pub fn video(&self) -> &[VideoBlock] {
        &self.video[..clamp(self.video_count, MAX_VIDEO_BLOCKS)]
    }
    /// The data areas shared with the driver's clients.
    pub fn areas(&self) -> &[ExportedArea] {
        &self.areas[..clamp(self.area_count, MAX_AREAS)]
    }

    /// The data area of the client whose call runs with stack pointer
    /// `stack_pointer`, if it has one.
    pub fn area_for_stack(&self, stack_pointer: u64) -> Option<ExportedArea> {
        self.areas()
            .iter()
            .find(|area| area.stack_bottom < stack_pointer && stack_pointer <= area.stack_end)
            .copied()
    }

    /// The functions of other components the driver imports.
    pub fn imports(&self) -> &[ImportRecord] {
        &self.imports[..clamp(self.import_count, MAX_IMPORTS)]
    }
    /// The component's bundled resource `index`, as bytes: Kickstart maps it
    /// read-only for the component's lifetime.
    pub fn resource(&self, index: usize) -> Option<&'static [u8]> {
        let record = self.resources[..clamp(self.resource_count, MAX_RESOURCES)].get(index)?;
        let len = usize::try_from(record.len).ok()?;
        // SAFETY: mapped read-only by Kickstart at `base` for `len` bytes.
        Some(unsafe { core::slice::from_raw_parts(record.base as *const u8, len) })
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

/// Width of a driver table's slot index (its tables are 256 entries).
const SLOT_BITS: u32 = 8;

/// A key to `slot` of the driver's own table, whose key is `table`, as
/// installed into a fresh slot (first incarnation): how a driver names what
/// its loader or a client put at a slot both agreed on.
pub fn local_key(table: u64, slot: u32) -> RawKey {
    let guard_bits = RawKey::from_wire(table).slot().0 & !((1 << SLOT_BITS) - 1);
    RawKey::new(KeySlot(guard_bits | slot), 1)
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

/// The data area of the client whose call this Thread is serving, found by
/// the stack it runs on (see [`ExportedArea`]).
///
/// Call it from an exported function's body, which runs on the caller's
/// stack.
#[cfg(target_arch = "aarch64")]
pub fn calling_area(init: &DriverInit) -> Option<ExportedArea> {
    let stack_pointer: u64;
    // SAFETY: reading `sp` has no side effects.
    unsafe {
        core::arch::asm!(
            "mov {}, sp",
            out(reg) stack_pointer,
            options(nomem, nostack, preserves_flags),
        );
    }
    init.area_for_stack(stack_pointer)
}

/// Lets one caller at a time into the part of a component that uses stacks
/// its imports reserve for it, such as a client of another component: those
/// stacks serve one Thread at a time (see `libmeddle_runtime::ExclusiveStacks`).
///
/// A component creates a static `Turnstile`, calls [`open`](Self::open) with an
/// `EventCount` it carved once it is set up, and advances
/// `DriverInit::event_count` to say it is ready. Every export then calls
/// [`enter`](Self::enter): it waits for readiness, then for every earlier
/// caller to leave.
pub struct Turnstile {
    turns: AtomicU64,
    tickets: AtomicU64,
}

/// The turn a caller holds in a [`Turnstile`]; leaving ends it.
pub struct Turn<'turnstile> {
    turns: EventCountKey,
    _turnstile: PhantomData<&'turnstile Turnstile>,
}

impl Turnstile {
    pub const fn new() -> Self {
        Self {
            turns: AtomicU64::new(0),
            tickets: AtomicU64::new(0),
        }
    }

    /// Count the turns on `turns`, an `EventCount` of the component's.
    pub fn open(&self, turns: RawKey) {
        self.turns.store(turns.to_wire(), Ordering::Release);
    }

    /// Wait until the component is ready and every earlier caller has left.
    pub fn enter(&self, init: &DriverInit) -> Turn<'_> {
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .await_ge(1, EventCountKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("turnstile: ready wait failed: {:?}", error.code()));
        let ticket = self.tickets.fetch_add(1, Ordering::AcqRel) + 1;
        let turns = EventCountKey::from_key(RawKey::from_wire(self.turns.load(Ordering::Acquire)));
        turns
            .await_ge(ticket - 1, EventCountKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("turnstile: turn wait failed: {:?}", error.code()));
        Turn {
            turns,
            _turnstile: PhantomData,
        }
    }
}

impl Default for Turnstile {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        self.turns
            .advance(1)
            .unwrap_or_else(|error| panic!("turnstile: turn release failed: {:?}", error.code()));
    }
}
