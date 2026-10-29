//! Driver matching: one instance of a bundled driver per enabled device tree
//! node that names one of its `compatible` strings.
//!
//! For each instance Kickstart builds a component `AddressSpace`, loads the
//! driver's image and hands it, through its init page
//! (`libdevice_driver::DriverInit`):
//!
//! - its register pages as page-sized device Untypeds carved out of the
//!   device Untypeds handed out at boot, or, for a page several matched nodes
//!   touch, a copy of one device Frame;
//! - one `IRQHandler` per interrupt line of the node, issued through the boot
//!   `IRQControl` and bound to the driver's Notification and Thread, interrupt
//!   *i* signalling bit *i*; a line the node lists more than once (the DMA
//!   node's line shared by channels 11 to 14) is issued once, at its first
//!   position (`libdevicetree::interrupts::distinct_interrupts`);
//! - the pins of the node's default pin state (`pinctrl-0`, see
//!   `libdevicetree::brcm_pins`), which the driver sets up through `gpio_v1`;
//! - for a DMA controller node, its `brcm,dma-channel-mask`: the channels the
//!   firmware leaves to the ARM;
//! - for a driver whose manifest entry says `video_memory = true`, the device
//!   Untypeds over the `VideoCore` memory the firmware allocates framebuffers
//!   in, outside every other Untyped (see `video_untypeds`);
//! - a RAM Untyped, with its physical base and the bus address the node's
//!   device reaches it at (`libdevicetree::dma_address`), a Time with the
//!   budget of its manifest entry, and one Thread configured on
//!   that Time and started at the driver's entry with the init page's address
//!   in `x0`;
//! - an `EventCount` for the driver's own Threads.
//!
//! Each further Thread the driver declares (`driver_threads!`) gets its own
//! stack and a Time with the same budget, and starts at its
//! entry with the init page's address in `x0`, right after the first. The
//! root dispatcher receives the priority each declares (the first Thread is
//! normal); the kernel never sees it.
//!
//! Once every instance is loaded, Kickstart wires their imports: for each
//! function a driver's `interfaces/imports.if` uses, it follows the `via`
//! properties from the driver's node to an instance exporting the interface,
//! creates the function's Invocation into the driver's table on a target stack
//! that instance reserves for this driver, and records it in the init page,
//! which also carries the driver's own Return key for the functions it
//! exports. Then the Threads start.
//!
//! A program, a `[[component]]` manifest entry with `program = true`, is a driver
//! without a device tree node: one instance of it loads, with no register
//! pages, interrupts or DMA view, and the import chains of its
//! `interfaces/imports.if` start from a `compatible` step. Its further Threads
//! (`driver_threads!`) are declared and started like a driver's.
//!
//! A node whose registers touch a page the kernel keeps is skipped, unless its
//! driver serves a compatible string on `privileged::SHARES_KERNEL_PAGES`: that
//! driver receives a device Frame minted for the page. Kickstart never calls
//! into a driver.
//!
//! Untypeds, Notifications, Times and `IRQHandler`s are outside the
//! `CopyDerive`/`Move` allowlist, so Kickstart grants them bootstrap-only: it
//! writes the boot-table entry into the driver's table and deletes its own.

use {
    crate::{
        bootstrap::{
            BootState, device_untypeds, interrupt_controllers, kernel_owned_device_ranges,
            video_untypeds,
        },
        device_tree,
        privileged::SHARES_KERNEL_PAGES,
        serial::SerialChannelParts,
    },
    libcomponent_loader::{
        builder::Builder,
        component::Component,
        keys::{BOOT_TABLE_GUARD, BOOT_TABLE_SIZE_BITS, SlotCursor, boot_key},
        loader::{StackRegion, update_init, write_init},
    },
    libdevice_driver::{
        DATA_WINDOW_VA, DEVICE_WINDOW_VA, DevicePage, DispatchInit, DriverInit, ExportedArea,
        FB_WINDOW_SPANS, INIT_VA, Interrupt, MAX_AREAS, MAX_DEVICE_PAGES, MAX_DISPATCHED_THREADS,
        MAX_FURTHER_THREADS, MAX_IMPORTS, MAX_INTERRUPTS, MAX_PINS, MAX_REGIONS, MAX_RESOURCES,
        MAX_VIDEO_BLOCKS, MapWindow, NO_BUS_ADDRESS, NO_PULL, PAGE_FRAME, PAGE_UNTYPED, PinSetting,
        Priority, Region, ResourceRecord, STACK_REGION, VideoBlock,
    },
    libdevicetree::{
        DevTreeIndex, Node, brcm_dma_channel_mask, brcm_pins, dma_address, find_compatible,
        interrupts::{MAX_INTERRUPT_CELLS, distinct_interrupts},
        is_compatible, is_enabled, linked_phandle, node_by_phandle, phandle, regions,
    },
    libimage::{DriverImage, Link, Provider},
    libmeddle_runtime::ImportRecord,
    libobject::{
        CapError, FrameKey, IrqControlKey, IrqHandlerKey, KeySlot, KeyTableKey, ObjectType,
        PlacementWindow, RawKey, Rights, StackPoolHandle, TimeKey, UntypedKey,
        address_space::AddressSpaceKey,
        irq::MAX_ISSUE_CELLS,
        scheduler::{SchedulerKey, ThreadStart},
        thread::ThreadReturnKey,
    },
    libqemu::semihosting as semi,
    nucleus::{
        api::key_entry::KeyEntry,
        objects::{ArchObjectsImpl, KeyTable, Nucleus},
    },
};

const PAGE: u64 = 4096;
const PAGE_BITS: u8 = 12;

/// The tables of a framebuffer window.
const FB_WINDOW_TABLES: u32 = 4;
const _: () = assert!(FB_WINDOW_TABLES as usize == FB_WINDOW_SPANS);

/// The interface whose importers get a framebuffer window.
const FB_INTERFACE: &str = "fb_v1";

/// The interface whose importers get a surface window.
const SURFACE_INTERFACE: &str = "compositor_v1";

/// The `compatible` string of the serial driver a program with `serial = true`
/// is attached to.
const SERIAL_COMPATIBLE: &str = "arm,pl011";

/// Most driver instances one boot loads.
pub const MAX_DRIVER_INSTANCES: usize = 16;

/// Pages of each driver Thread's stack: 32 KiB. `usb-core`'s Thread runs the
/// hub loop, an enumeration (its descriptor buffers), a trace line and the log
/// channel's send in one chain, which overflowed 16 KiB under QEMU.
const STACK_PAGES: u32 = 8;

/// First slot of a driver's table Kickstart fills: the slots below are the
/// well-known ones every component table has.
const FIRST_DRIVER_SLOT: u32 = 5;

/// Guard of the first driver instance's table; later instances count up.
const FIRST_DRIVER_GUARD: u32 = 0x0D_0000;

/// Guard of the first driver instance's archive table; later instances count
/// up. A key holds the guard above `size_bits` slot bits in 32 bits, so this
/// 14-bit guard leaves an archive up to 2^18 slots (the KeyTable limit is
/// 2^20): segments and resources up to 1 GiB, which the 32-bit key would
/// not allow with archive guards derived from the 20-bit driver guards.
const FIRST_ARCHIVE_GUARD: u32 = 0x3A00;

/// The boot keys driver loading draws on.
#[derive(Clone, Copy)]
pub struct BootKeys {
    pub self_table: RawKey,
    pub untyped: RawKey,
    pub boot_time: RawKey,
    pub root_scheduler: RawKey,
    pub irq_control: RawKey,
    pub boot_thread: RawKey,
}

impl BootKeys {
    pub fn of(boot: &BootState) -> Self {
        Self {
            self_table: boot.self_table_key,
            untyped: boot.boot_untyped_key,
            boot_time: boot.boot_time_key,
            root_scheduler: boot.root_scheduler_key,
            irq_control: boot.irq_control_key,
            boot_thread: boot.boot_thread_key,
        }
    }
}

/// A loaded driver instance, before its Threads start.
pub struct LoadedDriver<'a> {
    pub driver: &'static DriverImage,
    /// The node it drives
    pub node: &'a str,
    pub component: &'a Component,
    /// The driver's first Thread, the one its interrupts wake, in the boot
    /// table
    pub thread: RawKey,
    /// What the driver's init page holds
    pub init: &'a DriverInit,
    /// First slot of the driver's table nothing occupies
    pub first_free_slot: u32,
}

/// One planned instance: a driver and the node it serves.
#[derive(Clone)]
struct Instance {
    driver: &'static DriverImage,
    /// Position of the matched string in the driver's `compatible` list
    compatible_index: usize,
    /// The node it serves; none for a program, which has no device
    node: Option<Node<'static, 'static, 'static>>,
    regions: [Region; MAX_REGIONS],
    region_count: usize,
    pages: [u64; MAX_DEVICE_PAGES],
    page_count: usize,
}

impl Instance {
    /// An instance of a program: a driver without a device tree node.
    const fn program(driver: &'static DriverImage) -> Self {
        Self {
            driver,
            compatible_index: 0,
            node: None,
            regions: [Region { start: 0, size: 0 }; MAX_REGIONS],
            region_count: 0,
            pages: [0; MAX_DEVICE_PAGES],
            page_count: 0,
        }
    }

    /// The name of the node it serves, for messages.
    fn name(&self) -> &str {
        self.node
            .as_ref()
            .and_then(|node| node.name().ok())
            .unwrap_or("-")
    }

    fn pages(&self) -> &[u64] {
        &self.pages[..self.page_count]
    }
}

/// The physical pages `node`'s registers span, sorted and deduplicated, with
/// its register ranges; `None` if they do not fit the init record.
fn plan_instance(
    driver: &'static DriverImage,
    compatible_index: usize,
    node: &Node<'static, 'static, 'static>,
) -> Option<Instance> {
    let mut instance = Instance {
        driver,
        compatible_index,
        node: Some(node.clone()),
        regions: [Region::default(); MAX_REGIONS],
        region_count: 0,
        pages: [0; MAX_DEVICE_PAGES],
        page_count: 0,
    };
    for region in regions(node) {
        if region.size == 0 {
            continue;
        }
        *instance.regions.get_mut(instance.region_count)? = Region {
            start: region.start,
            size: region.size,
        };
        instance.region_count += 1;
        let mut page = region.start & !(PAGE - 1);
        while page < region.start + region.size {
            if !instance.pages().contains(&page) {
                *instance.pages.get_mut(instance.page_count)? = page;
                instance.page_count += 1;
            }
            page += PAGE;
        }
    }
    instance.pages[..instance.page_count].sort_unstable();
    Some(instance)
}

/// Whether `page` touches a range the kernel drives itself.
fn kernel_owned(page: u64) -> bool {
    kernel_owned_device_ranges()
        .iter()
        .any(|&(start, size)| page < start + size && start < page + PAGE)
}

/// Every enabled node some driver of `drivers` serves, the first matching
/// driver taking it.
fn plan(drivers: &[&'static DriverImage]) -> ([Option<Instance>; MAX_DRIVER_INSTANCES], usize) {
    let tree = device_tree::at_el1().index();
    let mut instances = [const { None }; MAX_DRIVER_INSTANCES];
    let mut count = 0;
    for node in tree.nodes() {
        if !is_enabled(&node) {
            continue;
        }
        let Some((driver, compatible_index)) = drivers.iter().find_map(|driver| {
            driver
                .compatible
                .iter()
                .position(|entry| is_compatible(&node, entry))
                .map(|index| (*driver, index))
        }) else {
            continue;
        };
        let name = node.name().unwrap_or("?");
        let Some(instance) = plan_instance(driver, compatible_index, &node) else {
            semi::println!(
                "🥾 Driver {}: {name} has too many registers, skipped",
                driver.component.name
            );
            continue;
        };
        let shares_kernel_pages = driver
            .compatible
            .get(compatible_index)
            .is_some_and(|compatible| SHARES_KERNEL_PAGES.contains(compatible));
        if !shares_kernel_pages && instance.pages().iter().any(|&page| kernel_owned(page)) {
            semi::println!(
                "🥾 Driver {}: {name} shares a page with the kernel, skipped",
                driver.component.name
            );
            continue;
        }
        if count == MAX_DRIVER_INSTANCES {
            semi::println!(
                "🥾 Driver {}: too many instances, {name} skipped",
                driver.component.name
            );
            continue;
        }
        instances[count] = Some(instance);
        count += 1;
    }
    // Programs have no node: one instance each.
    for driver in drivers.iter().filter(|driver| driver.compatible.is_empty()) {
        if count == MAX_DRIVER_INSTANCES {
            semi::println!(
                "🥾 Program {}: too many instances, skipped",
                driver.component.name
            );
            continue;
        }
        instances[count] = Some(Instance::program(driver));
        count += 1;
    }
    (instances, count)
}

/// How many instances [`load_drivers`] loads for `drivers`: one per enabled
/// node they serve, skipping the nodes it cannot load. For sizing the boot
/// pools before the nucleus exists.
pub fn instance_count(drivers: &[&'static DriverImage]) -> usize {
    plan(drivers).1
}

/// One piece of device memory Kickstart holds: an Untyped in the boot table
/// and how much of it is carved.
#[derive(Clone, Copy)]
struct Piece {
    key: RawKey,
    base: u64,
    size_bits: u8,
    used: u64,
}

impl Piece {
    fn end(&self) -> u64 {
        self.base + (1 << self.size_bits)
    }
}

/// Most device-memory pieces tracked while splitting.
const MAX_PIECES: usize = 64;

/// The device Untypeds handed out at boot, split into page-sized device
/// Untypeds on demand.
struct DeviceMemory {
    pieces: [Option<Piece>; MAX_PIECES],
}

impl DeviceMemory {
    fn new(builder: &Builder<'_>) -> Self {
        let mut memory = Self {
            pieces: [None; MAX_PIECES],
        };
        let range = device_untypeds();
        for slot in range.first_slot..range.first_slot + range.count {
            let key = boot_key(slot, 1);
            let region = *builder
                .entry(key)
                .as_untyped()
                .unwrap_or_else(|error| panic!("🥾 device Untyped {slot}: {:?}", error.code()));
            memory.add(Piece {
                key,
                base: region.paddr,
                size_bits: region.size_bits,
                used: u64::try_from(region.watermark_bytes()).unwrap_or(u64::MAX),
            });
        }
        memory
    }

    fn add(&mut self, piece: Piece) {
        let free = self
            .pieces
            .iter_mut()
            .find(|candidate| candidate.is_none())
            .expect("🥾 too many device memory pieces");
        *free = Some(piece);
    }

    /// A page-sized device Untyped over `page`, in a fresh boot slot: the
    /// smallest piece holding `page` uncarved is carved up to it, each skipped
    /// stretch becoming its own aligned piece for later pages.
    fn take_page(
        &mut self,
        page: u64,
        untyped_table: &KeyTableKey,
        slots: &mut SlotCursor,
    ) -> RawKey {
        let index = self
            .pieces
            .iter()
            .enumerate()
            .filter_map(|(index, piece)| piece.map(|piece| (index, piece)))
            .filter(|(_, piece)| piece.base + piece.used <= page && page < piece.end())
            .min_by_key(|(_, piece)| piece.size_bits)
            .map_or_else(
                || panic!("🥾 no device Untyped holds page {page:#x}"),
                |(index, _)| index,
            );
        loop {
            let mut piece = self.pieces[index].expect("piece present");
            let next = piece.base + piece.used;
            let (size_bits, is_page) = if next == page {
                (PAGE_BITS, true)
            } else {
                // The largest aligned block from `next` that stops at `page`.
                let alignment = next.trailing_zeros();
                let room = 63 - (page - next).leading_zeros();
                (
                    u8::try_from(alignment.min(room)).unwrap_or(PAGE_BITS),
                    false,
                )
            };
            let child = UntypedKey::from_key(piece.key)
                .retype(
                    ObjectType::UNTYPED,
                    size_bits,
                    0,
                    1,
                    untyped_table,
                    slots.take(1),
                    Rights::all(),
                )
                .unwrap_or_else(|error| {
                    panic!("🥾 device Untyped split failed: {:?}", error.code())
                });
            piece.used += 1 << size_bits;
            self.pieces[index] = Some(piece);
            if is_page {
                return child;
            }
            self.add(Piece {
                key: child,
                base: next,
                size_bits,
                used: 0,
            });
        }
    }
}

/// Install `entry` in `component`'s table at `slot`; the component-local key.
pub(crate) fn grant(component: &Component, entry: KeyEntry, slot: u32) -> RawKey {
    // SAFETY: the component's retained table, borrowed for this bootstrap
    // grant only; no capability invocation overlaps it.
    unsafe { &mut *KeyTable::at_carve(component.table_addr) }
        .insert(KeySlot(slot), entry, component.guard)
        .unwrap_or_else(|failure| panic!("🥾 driver grant failed: {:?}", failure.error.code()))
}

/// Move the boot-table capability `key` into `component`'s table at `slot`:
/// a bootstrap-only grant of the entry, then Delete of Kickstart's own.
fn move_to(
    builder: &Builder<'_>,
    self_table: &mut KeyTableKey,
    component: &Component,
    key: RawKey,
    slot: u32,
) -> RawKey {
    let local = grant(component, builder.entry(key), slot);
    self_table
        .delete(key)
        .unwrap_or_else(|error| panic!("🥾 boot-table Delete failed: {:?}", error.code()));
    local
}

/// Install `entry` in the boot table at `slot`.
fn install_boot(builder: &Builder<'_>, entry: KeyEntry, slot: u32) -> RawKey {
    // SAFETY: the live boot table, borrowed for this bootstrap grant only.
    unsafe { &mut *KeyTable::at_carve(builder.boot_table_addr) }
        .insert(KeySlot(slot), entry, BOOT_TABLE_GUARD)
        .unwrap_or_else(|failure| panic!("🥾 boot install failed: {:?}", failure.error.code()))
}

/// The node `links` lead to from `node` (none for a program, which has no
/// node, so its chain starts with `compatible`): a property step reads the
/// first cell of the current node's property as the phandle of the next node,
/// and a `compatible` first step starts from the one node with that string.
fn follow_links<'a, 'i, 'dt>(
    tree: &'a DevTreeIndex<'i, 'dt>,
    node: Option<&Node<'a, 'i, 'dt>>,
    links: &[Link],
) -> Option<Node<'a, 'i, 'dt>> {
    let mut current = node.cloned();
    for (index, link) in links.iter().enumerate() {
        current = Some(match *link {
            Link::Property(property) => {
                node_by_phandle(tree, linked_phandle(current.as_ref()?, property)?)?
            }
            Link::Compatible(compatible) if index == 0 => find_compatible(tree, &[compatible])?.0,
            Link::Compatible(_) => return None,
        });
    }
    current
}

/// Whether two views of the device tree name the same node.
fn same_node(first: &Node<'_, '_, '_>, second: &Node<'_, '_, '_>) -> bool {
    first.name().ok() == second.name().ok() && phandle(first) == phandle(second)
}

/// A provider for messages: a device tree chain, its steps separated by
/// commas, or a component name.
struct Provided(Provider);

impl core::fmt::Display for Provided {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Provider::Device(links) => {
                formatter.write_str("device ")?;
                for (index, link) in links.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(", ")?;
                    }
                    match link {
                        Link::Property(property) => formatter.write_str(property)?,
                        Link::Compatible(compatible) => {
                            write!(formatter, "compatible \"{compatible}\"")?;
                        }
                    }
                }
                Ok(())
            }
            Provider::Component(name) => write!(formatter, "component {name}"),
        }
    }
}

/// The one loaded instance of the manifest component `name` that exports
/// `interface` at `type_code`: `Ok(None)` when no loaded instance does, and
/// `Err` with their count when more than one does.
fn component_exporter(
    prepared: &[Option<Prepared<'_>>],
    name: &str,
    interface: &str,
    type_code: u64,
) -> Result<Option<usize>, usize> {
    let mut exporters = prepared
        .iter()
        .enumerate()
        .filter(|(_, candidate)| {
            candidate.as_ref().is_some_and(|candidate| {
                let component = candidate.instance.driver.component;
                component.name == name && component.exports_interface(interface, type_code)
            })
        })
        .map(|(index, _)| index);
    let first = exporters.next();
    let count = usize::from(first.is_some()) + exporters.count();
    match (first, count) {
        (None, _) => Ok(None),
        (Some(_), 1) => Ok(first),
        (Some(_), count) => Err(count),
    }
}

/// What one (client, exporter) pair shares: a data area, and the io-channel
/// that follows it, at one address in both.
#[derive(Clone, Copy, Debug, Default)]
struct PairArea {
    base: u64,
    /// Bytes of the data area (zero: none)
    data_bytes: u64,
    /// Base of the channel's control page (zero: no channel)
    channel_base: u64,
    /// Key of the channel's `EventCount` in the client's table (zero: none)
    channel_event: u64,
}

/// Bytes of the window data areas are mapped in: one 2 MiB span.
const DATA_WINDOW_BYTES: u64 = 2 * 1024 * 1024;

/// Map the `bytes` data area at `base` read/write in both `participants`,
/// from Frames carved from the boot Untyped. The first participant maps the
/// Frames themselves, the second a copy of each (a Frame capability tracks one
/// mapping); Kickstart keeps the capabilities, so neither participant can
/// unmap the area.
fn share_data_area(
    builder: &Builder<'_>,
    self_table: &KeyTableKey,
    slots: &mut SlotCursor,
    base: u64,
    bytes: u64,
    participants: [&Component; 2],
) {
    let pages = u32::try_from(bytes / PAGE).expect("a data area of a few pages");
    let first = slots.take(pages);
    let frames = builder.retype_frames(first, pages);
    let rights = Rights(Rights::READ | Rights::WRITE);
    let [first_participant, second_participant] = participants;
    Builder::map_frames(
        first,
        frames.incarnation(),
        pages,
        first_participant.address_space_key,
        base,
        rights,
    );
    for page in 0..pages {
        let original = boot_key(first + page, frames.incarnation());
        let copy = self_table
            .copy_derive(original, builder.self_table, slots.take(1), rights)
            .unwrap_or_else(|error| panic!("🥾 data area Frame copy failed: {:?}", error.code()));
        FrameKey::from_key(copy)
            .map(
                second_participant.address_space_key,
                base + u64::from(page) * PAGE,
                rights,
                libobject::FrameAttribute::Default,
            )
            .unwrap_or_else(|error| panic!("🥾 data area Frame.Map failed: {:?}", error.code()));
    }
}

/// Where the root dispatcher sees the Scheduler's record table.
const RECORD_VA: u64 = 0x3c00_0000;

/// Bytes between records: the nucleus's part of a record and nothing more.
const RECORD_STRIDE: u32 = 128;

/// Records a Frame holds.
const RECORDS_PER_FRAME: u32 = 4096 / RECORD_STRIDE;

/// Most Threads the root dispatcher manages: every driver instance's Threads.
const MAX_MANAGED_THREADS: usize = MAX_DRIVER_INSTANCES * (1 + MAX_FURTHER_THREADS);

const _: () = assert!(MAX_MANAGED_THREADS <= MAX_DISPATCHED_THREADS);

/// A Thread the root dispatcher dispatches, and its priority.
#[derive(Clone, Copy)]
struct ManagedThread {
    thread: RawKey,
    priority: Priority,
}

/// Slot of the dispatcher's table where the key of the Thread whose record is
/// in slot `s` sits at `FIRST_THREAD_KEY_SLOT + s`.
const FIRST_THREAD_KEY_SLOT: u32 = 256;

/// Bind the loaded component `dispatcher` as the root Scheduler's dispatcher.
///
/// The Scheduler's record table is shared first: records for `managed` in
/// slots 1 up, in Frames mapped read-only in the dispatcher's
/// `AddressSpace`. The dispatcher receives the Scheduler, one key to each
/// managed Thread and each one's priority, and where all of that is
/// (`DriverInit::dispatch`). Binding starts it and takes the managed Threads
/// off the kernel's queue.
fn bind_root_dispatcher(
    builder: &Builder<'_>,
    root_scheduler: &SchedulerKey,
    scheduler_key: RawKey,
    slots: &mut SlotCursor,
    dispatcher: &mut Prepared<'_>,
    managed: &[Option<ManagedThread>],
) {
    let slot_count = u32::try_from(managed.len()).expect("a few managed Threads");
    let frames = (slot_count + 1).div_ceil(RECORDS_PER_FRAME);
    let component = &dispatcher.component;
    Builder::map_table(
        builder.carve_tables(slots.take(1), 1),
        component.l2,
        RECORD_VA,
    );
    let first = slots.take(frames);
    let incarnation = builder.retype_frames(first, frames).incarnation();
    Builder::map_frames(
        first,
        incarnation,
        frames,
        component.address_space_key,
        RECORD_VA,
        Rights(Rights::READ),
    );
    for frame in 0..frames {
        root_scheduler
            .share_region(
                boot_key(first + frame, incarnation),
                frame * RECORDS_PER_FRAME,
                RECORD_STRIDE,
            )
            .unwrap_or_else(|error| panic!("🥾 ShareRegion failed: {:?}", error.code()));
    }
    let mut priorities = [Priority::NORMAL; MAX_DISPATCHED_THREADS];
    for ((slot, managed), priority) in (1..).zip(managed.iter().flatten()).zip(&mut priorities) {
        *priority = managed.priority;
        root_scheduler
            .assign_record(managed.thread, slot)
            .unwrap_or_else(|error| panic!("🥾 AssignRecord failed: {:?}", error.code()));
        let local = grant(
            component,
            builder.entry(managed.thread),
            FIRST_THREAD_KEY_SLOT + slot,
        );
        assert_eq!(
            local.incarnation(),
            1,
            "🥾 a fresh slot's first incarnation"
        );
    }
    let scheduler_slot = dispatcher.next_slot;
    dispatcher.next_slot += 1;
    let scheduler = grant(component, builder.entry(scheduler_key), scheduler_slot);
    update_init(dispatcher.init_page, |init: &mut DriverInit| {
        init.first_free_slot = u64::from(dispatcher.next_slot);
        init.dispatch = DispatchInit {
            scheduler: scheduler.to_wire(),
            records: RECORD_VA,
            stride: u64::from(RECORD_STRIDE),
            slot_count: u64::from(slot_count),
            first_thread_key_slot: u64::from(FIRST_THREAD_KEY_SLOT),
            priorities,
        };
    });
    root_scheduler
        .bind_dispatcher(dispatcher.thread)
        .unwrap_or_else(|error| panic!("🥾 BindDispatcher failed: {:?}", error.code()));
    semi::println!("🥾 Root dispatcher bound over {slot_count} Threads");
}

/// Pages of each target stack of the pool an exporting driver serves calls on.
const EXPORT_STACK_PAGES: u32 = 4;

/// Stacks of that pool: calls into the driver that can run at once.
const EXPORT_STACKS: u32 = 16;

/// An instance loaded but not started: what wiring its imports and starting
/// it need.
struct Prepared<'p> {
    instance: &'p Instance,
    component: Component,
    /// The driver's guarded stack span, which also holds the stack pool
    /// of the functions it exports
    stacks: StackRegion,
    /// The stack pool of the driver's exports, made when the first client
    /// is linked
    export_pool: Option<StackPoolHandle>,
    /// Physical address of the init page
    init_page: u64,
    thread: RawKey,
    further_threads: [Option<RawKey>; MAX_FURTHER_THREADS],
    /// First slot of the driver's table nothing occupies
    next_slot: u32,
}

/// Load one instance of the bundled `drivers` per enabled device tree node
/// they serve, wire every instance's imports, then, instance by instance,
/// call `before_start` and start its Threads. Boot-table slots come from
/// `slots`. Returns how many instances started.
pub fn load_drivers(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    builder: &Builder<'_>,
    keys: BootKeys,
    drivers: &[&'static DriverImage],
    slots: &mut SlotCursor,
    mut before_start: impl FnMut(&LoadedDriver<'_>),
) -> usize {
    let (instances, count) = plan(drivers);
    let instances = &instances[..count];
    let mut prepared: [Option<Prepared<'_>>; MAX_DRIVER_INSTANCES] =
        [const { None }; MAX_DRIVER_INSTANCES];
    let mut self_table = KeyTableKey::from_key(keys.self_table);
    let mut memory = DeviceMemory::new(builder);
    let controllers = interrupt_controllers();
    let irq_control = IrqControlKey::from_key(keys.irq_control);
    let root_scheduler = SchedulerKey::from_key(keys.root_scheduler);
    let boot_time = TimeKey::from_key(keys.boot_time);
    let untyped = UntypedKey::from_key(keys.untyped);

    // A page several instances touch becomes one device Frame they all copy.
    let instances_touching = |page: u64| {
        instances
            .iter()
            .flatten()
            .filter(|instance| instance.pages().contains(&page))
            .count()
    };
    // Whether the `VideoCore` memory went to a driver already.
    let mut video_given = false;
    let mut shared: [Option<(u64, RawKey)>; MAX_DRIVER_INSTANCES * MAX_DEVICE_PAGES] =
        [None; MAX_DRIVER_INSTANCES * MAX_DEVICE_PAGES];

    for (number, instance) in instances.iter().flatten().enumerate() {
        let driver = instance.driver;
        let image = driver.component;
        let name = instance.name();
        let guard = FIRST_DRIVER_GUARD + u32::try_from(number).unwrap_or(0);
        let component = builder.component(guard, slots);
        let archive_guard = FIRST_ARCHIVE_GUARD + u32::try_from(number).unwrap_or(0);
        builder.load_component(&component, image, archive_guard, slots);
        let mut stacks = builder.stack_region(&component, STACK_REGION, slots);
        let stack = builder.user_stack(&mut stacks, STACK_PAGES, slots);
        assert!(
            driver.threads.len() <= MAX_FURTHER_THREADS,
            "🥾 driver {} declares too many Threads",
            image.name
        );
        let mut further_stack_tops = [0; MAX_FURTHER_THREADS];
        for top in further_stack_tops.iter_mut().take(driver.threads.len()) {
            *top = builder.user_stack(&mut stacks, STACK_PAGES, slots).top;
        }
        let init_page =
            builder.private_pages(component.address_space_key, component.l2, INIT_VA, 1, slots);
        Builder::map_table(
            builder.carve_tables(slots.take(1), 1),
            component.l2,
            DEVICE_WINDOW_VA,
        );
        let driver_table = component.table();
        let mut next_slot = FIRST_DRIVER_SLOT;
        let mut local_slot = || {
            let slot = next_slot;
            next_slot += 1;
            slot
        };
        let mut init = DriverInit::EMPTY;
        init.address_space = component
            .local_key(KeySlot::SELF_ADDRESS_SPACE.0, 1)
            .to_wire();
        init.table = component.local_key(KeySlot::SELF_KEYTABLE.0, 1).to_wire();
        init.return_key = ThreadReturnKey::provisioned(guard, BOOT_TABLE_SIZE_BITS)
            .raw()
            .to_wire();
        // ── Registers ───────────────────────────────────────────────────
        assert!(
            image.resources.len() <= MAX_RESOURCES,
            "🥾 component {} names too many resources",
            image.name
        );
        init.resource_count = image.resources.len() as u64;
        for (record, mapping) in init.resources.iter_mut().zip(image.resources) {
            *record = ResourceRecord {
                base: mapping.virt_addr,
                len: mapping.size,
            };
        }
        init.compatible_index = instance.compatible_index as u64;
        init.region_count = instance.region_count as u64;
        init.regions = instance.regions;
        for (record, &page) in init.pages.iter_mut().zip(instance.pages()) {
            *record = if kernel_owned(page) {
                // Only a driver on `SHARES_KERNEL_PAGES` gets here: no device
                // Untyped covers the page, so the Frame is minted directly.
                semi::println!(
                    "🥾 Driver {}: {name} receives the kernel-owned page {page:#x}",
                    image.name
                );
                DevicePage {
                    paddr: page,
                    key: grant(
                        &component,
                        KeyEntry::new_frame(
                            page,
                            PAGE_BITS,
                            true,
                            Rights(Rights::READ | Rights::WRITE),
                        ),
                        local_slot(),
                    )
                    .to_wire(),
                    kind: PAGE_FRAME,
                }
            } else if instances_touching(page) > 1 {
                let frame = match shared.iter().flatten().find(|(known, _)| *known == page) {
                    Some(&(_, frame)) => frame,
                    None => {
                        let page_untyped = memory.take_page(page, builder.self_table, slots);
                        let frame = UntypedKey::from_key(page_untyped)
                            .retype(
                                ObjectType::FRAME,
                                PAGE_BITS,
                                0,
                                1,
                                builder.self_table,
                                slots.take(1),
                                Rights::all(),
                            )
                            .unwrap_or_else(|error| {
                                panic!("🥾 shared device Frame failed: {:?}", error.code())
                            });
                        *shared
                            .iter_mut()
                            .find(|entry| entry.is_none())
                            .expect("shared slots") = Some((page, frame));
                        frame
                    }
                };
                let copy = self_table
                    .copy_derive(
                        frame,
                        &driver_table,
                        local_slot(),
                        Rights(Rights::READ | Rights::WRITE),
                    )
                    .unwrap_or_else(|error| {
                        panic!("🥾 device Frame copy failed: {:?}", error.code())
                    });
                DevicePage {
                    paddr: page,
                    key: copy.to_wire(),
                    kind: PAGE_FRAME,
                }
            } else {
                let page_untyped = memory.take_page(page, builder.self_table, slots);
                DevicePage {
                    paddr: page,
                    key: move_to(
                        builder,
                        &mut self_table,
                        &component,
                        page_untyped,
                        local_slot(),
                    )
                    .to_wire(),
                    kind: PAGE_UNTYPED,
                }
            };
        }
        init.page_count = instance.page_count as u64;

        // ── Memory, Time and Thread ─────────────────────────────────────
        let ram = untyped
            .retype(
                ObjectType::UNTYPED,
                driver.memory_bits,
                0,
                1,
                builder.self_table,
                slots.take(1),
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("🥾 driver memory Retype failed: {:?}", error.code()));
        let memory_paddr = builder
            .entry(ram)
            .as_untyped()
            .unwrap_or_else(|error| panic!("🥾 driver memory region: {:?}", error.code()))
            .paddr;
        init.memory_paddr = memory_paddr;
        init.memory_bus = instance
            .node
            .as_ref()
            .and_then(|node| dma_address(node, memory_paddr, 1 << driver.memory_bits))
            .unwrap_or(NO_BUS_ADDRESS);
        init.memory = move_to(builder, &mut self_table, &component, ram, local_slot()).to_wire();
        init.memory_size = 1 << driver.memory_bits;
        if driver.video_memory {
            // The blocks move to the first instance that asks for them.
            let video = video_untypeds();
            let blocks = if video_given { 0 } else { video.count };
            for slot in video.first_slot..video.first_slot + blocks {
                let key = boot_key(slot, 1);
                let region = *builder
                    .entry(key)
                    .as_untyped()
                    .unwrap_or_else(|error| panic!("🥾 video Untyped {slot}: {:?}", error.code()));
                let index = usize::try_from(init.video_count).unwrap_or(MAX_VIDEO_BLOCKS);
                assert!(index < MAX_VIDEO_BLOCKS, "🥾 too many video memory blocks");
                init.video[index] = VideoBlock {
                    key: move_to(builder, &mut self_table, &component, key, local_slot()).to_wire(),
                    base: region.paddr,
                    size_bits: u64::from(region.size_bits),
                };
                init.video_count += 1;
            }
            video_given |= blocks != 0;
        }
        let mut retype = |kind: ObjectType| {
            untyped
                .retype(
                    kind,
                    0,
                    0,
                    1,
                    builder.self_table,
                    slots.take(1),
                    Rights::all(),
                )
                .unwrap_or_else(|error| {
                    panic!("🥾 driver {kind:?} Retype failed: {:?}", error.code())
                })
        };
        // A Thread on a Time of its own, starting at `entry` on `stack_top`;
        // the Thread and the Time.
        let mut configure_thread = |entry: u64, stack_top: u64| {
            let thread = retype(ObjectType::THREAD);
            let time = retype(ObjectType::TIME);
            boot_time
                .transfer(&TimeKey::from_key(time), driver.budget_ns)
                .unwrap_or_else(|error| {
                    panic!("🥾 driver Time.Transfer failed: {:?}", error.code())
                });
            root_scheduler
                .configure_thread(
                    thread,
                    component.address_space_key,
                    ThreadStart {
                        entry,
                        stack_pointer: stack_top,
                        argument: INIT_VA,
                    },
                    time,
                )
                .unwrap_or_else(|error| {
                    panic!("🥾 driver ConfigureThread failed: {:?}", error.code())
                });
            (thread, time)
        };
        let (thread, time) =
            configure_thread(image.entry.expect("a driver has an entry"), stack.top);
        let mut further_threads = [None; MAX_FURTHER_THREADS];
        for ((record, declared), &stack_top) in further_threads
            .iter_mut()
            .zip(driver.threads)
            .zip(&further_stack_tops)
        {
            *record = Some(configure_thread(declared.entry, stack_top).0);
        }
        let notification = retype(ObjectType::NOTIFICATION);
        let event_count = retype(ObjectType::EVENT_COUNT);
        init.time = grant(&component, builder.entry(time), local_slot()).to_wire();
        init.event_count = move_to(
            builder,
            &mut self_table,
            &component,
            event_count,
            local_slot(),
        )
        .to_wire();

        // ── Interrupts ──────────────────────────────────────────────────
        let tree = device_tree::at_el1().index();
        let notification_id = builder
            .entry(notification)
            .carved_id()
            .unwrap_or_else(|error| panic!("🥾 Notification identity: {:?}", error.code()));
        let mut interrupt_count = 0;
        // A line the node lists more than once (channels sharing it) is
        // issued once, at its first position.
        for interrupt in instance
            .node
            .iter()
            .flat_map(|node| distinct_interrupts(tree, node))
            .take(MAX_INTERRUPTS)
        {
            let Some(controller) = controllers.index_of(interrupt.controller) else {
                semi::println!(
                    "🥾 Driver {}: {name} has an interrupt on a controller the kernel does not drive",
                    image.name
                );
                continue;
            };
            if interrupt.cells().len() > MAX_ISSUE_CELLS.min(MAX_INTERRUPT_CELLS) {
                continue;
            }
            let issued = match irq_control.issue(
                builder.self_table,
                slots.take(1),
                controller,
                interrupt.cells(),
            ) {
                Ok(issued) => issued,
                // Another node's driver holds the line already.
                Err(CapError::IrqLineActive { line }) => {
                    semi::println!(
                        "🥾 Driver {}: {name}'s interrupt line {line} is taken, skipped",
                        image.name
                    );
                    continue;
                }
                Err(error) => panic!("🥾 driver IRQControl.Issue failed: {:?}", error.code()),
            };
            let bits = 1_u64 << interrupt_count;
            let badged = install_boot(
                builder,
                KeyEntry::new_carved(notification_id, Rights::all(), bits),
                slots.take(1),
            );
            IrqHandlerKey::from_key(issued.handler)
                .set_notification(badged, thread)
                .unwrap_or_else(|error| {
                    panic!("🥾 driver SetNotification failed: {:?}", error.code())
                });
            self_table
                .delete(badged)
                .unwrap_or_else(|error| panic!("🥾 boot-table Delete failed: {:?}", error.code()));
            init.interrupts[interrupt_count] = Interrupt {
                handler: move_to(
                    builder,
                    &mut self_table,
                    &component,
                    issued.handler,
                    local_slot(),
                )
                .to_wire(),
                bits,
                line: u64::from(issued.line),
            };
            interrupt_count += 1;
        }
        init.interrupt_count = interrupt_count as u64;

        // ── Pins: the node's default pin state, for the gpio component ──
        let mut pin_count = 0;
        for pin in instance
            .node
            .iter()
            .flat_map(|node| brcm_pins(tree, node))
            .take(MAX_PINS)
        {
            init.pins[pin_count] = PinSetting {
                pin: pin.pin,
                function: pin.function,
                pull: pin.pull.unwrap_or(NO_PULL),
            };
            pin_count += 1;
        }
        init.pin_count = pin_count as u64;
        // ── A DMA controller's channels left to the ARM ─────────────────
        init.dma_channel_mask = instance
            .node
            .as_ref()
            .and_then(brcm_dma_channel_mask)
            .map_or(0, u64::from);
        init.notification = move_to(
            builder,
            &mut self_table,
            &component,
            notification,
            local_slot(),
        )
        .to_wire();
        write_init(init_page, init);
        prepared[number] = Some(Prepared {
            instance,
            component,
            stacks,
            export_pool: None,
            init_page,
            thread,
            further_threads,
            next_slot,
        });
    }
    let prepared = &mut prepared[..count];

    // ── Imports ─────────────────────────────────────────────────────────
    let tree = device_tree::at_el1().index();
    // The data area of each pair whose interface has one, as `(base, len)`,
    // the next free address of the window they share, and which instances
    // already have its page table installed.
    let mut call_areas = [[None::<PairArea>; MAX_DRIVER_INSTANCES]; MAX_DRIVER_INSTANCES];
    // A slot of Kickstart's own table to make an import key in before it is
    // badged into the client's table.
    let scratch_slot = slots.take(1);
    // The placement window of each pair with a function lending a key: slots
    // of the exporter's table reserved for the keys the client lends it.
    let mut call_windows = [[None::<PlacementWindow>; MAX_DRIVER_INSTANCES]; MAX_DRIVER_INSTANCES];
    let mut data_cursor = DATA_WINDOW_VA;
    let mut window_installed = [false; MAX_DRIVER_INSTANCES];
    for client in 0..count {
        let Some(client_instance) = prepared[client].as_ref().map(|loaded| loaded.instance) else {
            continue;
        };
        let client_image = client_instance.driver.component;
        let client_name = client_instance.name();
        // Imports of one interface are consecutive: report a missing link once.
        let mut unlinked: Option<&str> = None;
        for import in client_image.imports {
            let exporter = match import.provider {
                Provider::Device(links) => {
                    let target = follow_links(tree, client_instance.node.as_ref(), links);
                    Ok(prepared.iter().position(|candidate| {
                        candidate.as_ref().is_some_and(|candidate| {
                            target.as_ref().is_some_and(|target| {
                                candidate
                                    .instance
                                    .node
                                    .as_ref()
                                    .is_some_and(|node| same_node(node, target))
                            }) && candidate
                                .instance
                                .driver
                                .component
                                .exports_interface(import.interface, import.type_code)
                        })
                    }))
                }
                Provider::Component(name) => {
                    component_exporter(prepared, name, import.interface, import.type_code)
                }
            };
            let exporter = match exporter {
                Ok(Some(exporter)) => exporter,
                Ok(None) => {
                    if unlinked == Some(import.interface) {
                        continue;
                    }
                    unlinked = Some(import.interface);
                    semi::println!(
                        "🥾 Driver {}: {client_name} links no instance exporting {} through `{}`",
                        client_image.name,
                        import.interface,
                        Provided(import.provider)
                    );
                    continue;
                }
                Err(count) => {
                    // Not granted: the client's call reports it, and naming a
                    // different provider in the manifest is the fix.
                    semi::println!(
                        "🥾 Driver {}: {client_name} imports {} through `{}`, which {count} loaded instances export",
                        client_image.name,
                        import.interface,
                        Provided(import.provider)
                    );
                    continue;
                }
            };
            let export_pool = {
                let exporting = prepared[exporter].as_mut().expect("a loaded exporter");
                match exporting.export_pool {
                    Some(pool) => pool,
                    None => {
                        let pool = builder.stack_pool(
                            &mut exporting.stacks,
                            EXPORT_STACK_PAGES,
                            EXPORT_STACKS,
                            slots,
                        );
                        update_init(exporting.init_page, |init: &mut DriverInit| {
                            init.export_pool = pool.to_wire();
                        });
                        exporting.export_pool = Some(pool);
                        pool
                    }
                }
            };
            let area = if import.data_bytes == 0 && import.channel_pages == 0 {
                None
            } else if let Some(area) = call_areas[client][exporter] {
                Some(area)
            } else {
                // The data area first, then the io-channel: its control page and
                // data pages.
                let channel_bytes = if import.channel_pages == 0 {
                    0
                } else {
                    (1 + import.channel_pages) * PAGE
                };
                let bytes = import.data_bytes + channel_bytes;
                let base = data_cursor;
                data_cursor += bytes;
                assert!(
                    data_cursor <= DATA_WINDOW_VA + DATA_WINDOW_BYTES,
                    "🥾 the data window is full"
                );
                // A channel gets an EventCount the exporter advances after
                // completing packets and the client waits on: one key for
                // each, narrowed to what it does.
                let (exporter_event, client_event) = if import.channel_pages == 0 {
                    (0, 0)
                } else {
                    let event = builder
                        .untyped
                        .retype(
                            ObjectType::EVENT_COUNT,
                            0,
                            0,
                            1,
                            builder.self_table,
                            slots.take(1),
                            Rights::all(),
                        )
                        .unwrap_or_else(|error| {
                            panic!("🥾 channel EventCount Retype failed: {:?}", error.code())
                        });
                    let mut place = |index: usize, rights: u8| {
                        let loaded = prepared[index].as_mut().expect("a loaded instance");
                        let slot = loaded.next_slot;
                        loaded.next_slot += 1;
                        grant(
                            &loaded.component,
                            builder.entry(event).derive(Rights(rights)),
                            slot,
                        )
                        .to_wire()
                    };
                    (place(exporter, Rights::SEND), place(client, Rights::RECV))
                };
                let calling = prepared[client].as_ref().expect("a loaded client");
                let exporting = prepared[exporter].as_ref().expect("a loaded exporter");
                for (index, component) in [
                    (exporter, &exporting.component),
                    (client, &calling.component),
                ] {
                    if !window_installed[index] {
                        Builder::map_table(
                            builder.carve_tables(slots.take(1), 1),
                            component.l2,
                            DATA_WINDOW_VA,
                        );
                        window_installed[index] = true;
                    }
                }
                share_data_area(
                    builder,
                    &self_table,
                    slots,
                    base,
                    bytes,
                    [&exporting.component, &calling.component],
                );
                update_init(exporting.init_page, |init: &mut DriverInit| {
                    let index = usize::try_from(init.area_count).unwrap_or(MAX_AREAS);
                    let record = init.areas.get_mut(index).unwrap_or_else(|| {
                        panic!(
                            "🥾 driver {} serves more than {MAX_AREAS} data areas",
                            exporting.instance.driver.component.name
                        )
                    });
                    *record = ExportedArea {
                        base,
                        len: import.data_bytes,
                        channel_base: if import.channel_pages == 0 {
                            0
                        } else {
                            base + import.data_bytes
                        },
                        channel_event: exporter_event,
                    };
                    init.area_count += 1;
                });
                let pair = PairArea {
                    base,
                    data_bytes: import.data_bytes,
                    channel_base: if import.channel_pages == 0 {
                        0
                    } else {
                        base + import.data_bytes
                    },
                    channel_event: client_event,
                };
                call_areas[client][exporter] = Some(pair);
                Some(pair)
            };
            // A function that lends a key gets its pair's window: reserved once,
            // never reused, so a lent key never lands on another of the
            // exporter's keys. A function that lends nothing accepts no key.
            let window = import.lends.then(|| match call_windows[client][exporter] {
                Some(window) => window,
                None => {
                    let slots = u32::try_from(import.placement_slots)
                        .ok()
                        .filter(|&slots| slots > 0)
                        .unwrap_or_else(|| {
                            panic!(
                                "🥾 `{}` lends a key without a placement window",
                                import.export
                            )
                        });
                    let exporting = prepared[exporter].as_mut().expect("a loaded exporter");
                    let window = PlacementWindow::new(exporting.next_slot, slots);
                    exporting.next_slot += slots;
                    call_windows[client][exporter] = Some(window);
                    window
                }
            });
            let PairArea {
                base: area_base,
                data_bytes: area_len,
                channel_base,
                channel_event,
            } = area.unwrap_or_default();
            let exporting = prepared[exporter].as_ref().expect("a loaded exporter");
            let exporter_space = AddressSpaceKey::from_key(exporting.component.address_space_key);
            let entry = exporting
                .instance
                .driver
                .component
                .export(import.export)
                .unwrap_or_else(|| {
                    panic!(
                        "🥾 {} exports no `{}`",
                        exporting.instance.driver.component.name, import.export
                    )
                });
            let calling = prepared[client].as_mut().expect("a loaded client");
            let slot = calling.next_slot;
            calling.next_slot += 1;
            // The key the client holds carries the identity of its pair as its
            // badge (the base of the pair's shared memory, which is nonzero), so
            // the exporter knows its caller from the key and not from where the
            // call runs. A pair without shared memory gets an unbadged key. The
            // badge is set by deriving from a key Kickstart makes first in its
            // own table; the client gets `CALL` only, and a badged key cannot be
            // badged again whatever its rights.
            let client_table = calling.component.table();
            let created = exporter_space
                .create_invocation_with_window(
                    entry,
                    if area_base == 0 {
                        &client_table
                    } else {
                        &self_table
                    },
                    KeySlot(if area_base == 0 { slot } else { scratch_slot }),
                    export_pool,
                    window.unwrap_or(PlacementWindow::NONE),
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "🥾 CreateInvocation `{}` failed: {:?}",
                        import.export,
                        error.code()
                    )
                });
            let invocation = if area_base == 0 {
                created
            } else {
                let badged = self_table
                    .copy_derive_badged(
                        created,
                        &calling.component.table(),
                        slot,
                        Rights(Rights::CALL),
                        area_base,
                    )
                    .unwrap_or_else(|error| {
                        panic!(
                            "🥾 badging the import `{}` failed: {:?}",
                            import.export,
                            error.code()
                        )
                    });
                self_table.delete(created).unwrap_or_else(|error| {
                    panic!("🥾 deleting the unbadged import failed: {:?}", error.code())
                });
                badged
            };
            update_init(calling.init_page, |init: &mut DriverInit| {
                let index = usize::try_from(init.import_count).unwrap_or(MAX_IMPORTS);
                let record = init.imports.get_mut(index).unwrap_or_else(|| {
                    panic!(
                        "🥾 driver {} imports more than {MAX_IMPORTS} functions",
                        client_image.name
                    )
                });
                *record = ImportRecord {
                    type_code: import.type_code,
                    function: import.function,
                    invocation: invocation.to_wire(),
                    area_base,
                    area_len,
                    channel_base,
                    channel_event,
                };
                init.import_count += 1;
            });
        }
    }

    // ── Serial channel ──────────────────────────────────────────────────
    // The program that asks for it (`serial = true`) is attached to the PL011
    // driver's channel, before the driver's Threads start. There is no general
    // way to connect a client to a serial driver yet: the first such program
    // gets the first PL011 instance.
    let client = prepared.iter().position(|loaded| {
        loaded
            .as_ref()
            .is_some_and(|loaded| loaded.instance.driver.serial)
    });
    if let Some(client) = client {
        let uart = prepared.iter().position(|loaded| {
            loaded
                .as_ref()
                .is_some_and(|loaded| loaded.instance.driver.serves(SERIAL_COMPATIBLE))
        });
        match uart {
            Some(uart) => {
                let parts = SerialChannelParts::create(builder, slots);
                let driver = prepared[uart].as_ref().expect("a loaded PL011 driver");
                parts.attach_driver(builder, &driver.component);
                let program = prepared[client].as_mut().expect("a loaded client");
                let keys = parts.attach_client(builder, &program.component, &mut program.next_slot);
                update_init(program.init_page, |init: &mut DriverInit| {
                    init.serial = keys;
                });
            }
            None => {
                semi::println!(
                    "🥾 Program {} asks for a serial channel and no PL011 driver is loaded",
                    prepared[client].as_ref().map_or("?", |loaded| loaded
                        .instance
                        .driver
                        .component
                        .name)
                );
            }
        }
    }

    // A component that imports `fb_v1` maps the framebuffer's Frames itself:
    // it gets its second-level page table and spare last-level tables for the
    // spans that end up holding 4 KiB Frames. A component that imports
    // `compositor_v1` maps surfaces made of 2 MiB Frames, which need the
    // second-level table only.
    for loaded in prepared.iter_mut().flatten() {
        let imports = loaded.instance.driver.component.imports;
        let imports_interface = |name: &str| imports.iter().any(|import| import.interface == name);
        let maps_framebuffer = imports_interface(FB_INTERFACE);
        if !maps_framebuffer && !imports_interface(SURFACE_INTERFACE) {
            continue;
        }
        let mut window = MapWindow {
            l2: 0,
            tables: [0; FB_WINDOW_SPANS],
        };
        if maps_framebuffer {
            let first = slots.take(FB_WINDOW_TABLES);
            let tables = builder.carve_tables(first, FB_WINDOW_TABLES);
            for (index, table) in window.tables.iter_mut().enumerate() {
                let key = boot_key(
                    first + u32::try_from(index).unwrap_or(0),
                    tables.incarnation(),
                );
                *table = move_to(
                    builder,
                    &mut self_table,
                    &loaded.component,
                    key,
                    loaded.next_slot,
                )
                .to_wire();
                loaded.next_slot += 1;
            }
        }
        // The loader no longer needs the component's second-level table.
        window.l2 = move_to(
            builder,
            &mut self_table,
            &loaded.component,
            loaded.component.l2,
            loaded.next_slot,
        )
        .to_wire();
        loaded.next_slot += 1;
        update_init(loaded.init_page, |init: &mut DriverInit| {
            init.map_window = window;
        });
    }

    // ── Start ───────────────────────────────────────────────────────────
    let mut started = 0;
    // The Threads the root dispatcher dispatches, and the dispatcher itself.
    let mut managed = [None; MAX_MANAGED_THREADS];
    let mut managed_count = 0;
    let mut dispatcher = None;
    for (number, loaded) in prepared.iter_mut().enumerate() {
        let Some(loaded) = loaded else { continue };
        let instance = loaded.instance;
        let name = instance.name();
        let first_free_slot = loaded.next_slot;
        let init = update_init(loaded.init_page, |init: &mut DriverInit| {
            init.first_free_slot = u64::from(first_free_slot);
            *init
        });
        semi::println!(
            "🥾 Driver {} for {name}: {} register pages, {} interrupts, {} imports",
            instance.driver.component.name,
            init.page_count,
            init.interrupt_count,
            init.import_count
        );
        if instance.driver.dispatcher {
            // Bound to the Scheduler once every other Thread is known. It
            // is infrastructure, not a driver: the caller's hook never sees it.
            dispatcher = Some(number);
            started += 1;
            continue;
        }
        before_start(&LoadedDriver {
            driver: instance.driver,
            node: name,
            component: &loaded.component,
            thread: loaded.thread,
            init: &init,
            first_free_slot,
        });
        // The first Thread is normal; the further ones as they declare.
        let priorities = core::iter::once(Priority::NORMAL).chain(
            instance
                .driver
                .threads
                .iter()
                .map(|declared| Priority(declared.priority)),
        );
        for (thread, priority) in core::iter::once(loaded.thread)
            .chain(loaded.further_threads.into_iter().flatten())
            .zip(priorities)
        {
            root_scheduler
                .start_thread(thread)
                .unwrap_or_else(|error| panic!("🥾 driver StartThread failed: {:?}", error.code()));
            managed[managed_count] = Some(ManagedThread { thread, priority });
            managed_count += 1;
        }
        started += 1;
    }
    if let Some(number) = dispatcher {
        // The boot Thread is a Thread of the root Scheduler too: it is
        // dispatched like the others. (It usually parks for good; a test
        // kernel's boot Thread goes on running the test.)
        if let Some(root) = builder.entry(keys.root_scheduler).carved_id().ok()
            && let Some(boot) = nucleus.current_thread
            && managed_count < MAX_MANAGED_THREADS
        {
            nucleus
                .thread_mut(boot)
                .expect("the boot Thread exists")
                .scheduler = Some(root);
            nucleus.join_scheduler(boot, root);
            managed[managed_count] = Some(ManagedThread {
                thread: keys.boot_thread,
                priority: Priority::NORMAL,
            });
            managed_count += 1;
        }
        let loaded = prepared[number].as_mut().expect("a loaded dispatcher");
        bind_root_dispatcher(
            builder,
            &root_scheduler,
            keys.root_scheduler,
            slots,
            loaded,
            &managed[..managed_count],
        );
    }
    started
}
