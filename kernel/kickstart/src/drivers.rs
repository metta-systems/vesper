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
//! - one `IRQHandler` per interrupt of the node, issued through the boot
//!   `IRQControl` and bound to the driver's Notification and Thread, interrupt
//!   *i* signalling bit *i*;
//! - for a driver whose manifest entry says `video_memory = true`, the device
//!   Untypeds over the `VideoCore` memory the firmware allocates framebuffers
//!   in, outside every other Untyped (see `video_untypeds`);
//! - a RAM Untyped, with its physical base and the bus address the node's
//!   device reaches it at (`libdevicetree::dma_address`), a Time with the
//!   budget and priority of its manifest entry, and one Thread configured on
//!   that Time and started at the driver's entry with the init page's address
//!   in `x0`;
//! - an `EventCount` for the driver's own Threads.
//!
//! Each further Thread the driver declares (`driver_threads!`) gets its own
//! stack and a Time with the same budget and priority, and starts at its
//! entry with the init page's address in `x0`, right after the first.
//!
//! Once every instance is loaded, Kickstart wires their imports: for each
//! function a driver's `interfaces/imports.if` uses, it follows the `via`
//! properties from the driver's node to an instance exporting the interface,
//! creates the function's Invocation into the driver's table on a target stack
//! that instance reserves for this driver, and records it in the init page,
//! which also carries the driver's own Return key for the functions it
//! exports. Then the Threads start.
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
    },
    libcomponent_loader::{
        builder::Builder,
        component::Component,
        keys::{BOOT_TABLE_GUARD, BOOT_TABLE_SIZE_BITS, SlotCursor, boot_key},
        loader::{StackRegion, UserStack, update_init, write_init},
    },
    libdevice_driver::{
        DATA_WINDOW_VA, DEVICE_WINDOW_VA, DevicePage, DriverInit, ExportedArea, INIT_VA, Interrupt,
        MAX_AREAS, MAX_DEVICE_PAGES, MAX_FURTHER_THREADS, MAX_IMPORTS, MAX_INTERRUPTS, MAX_REGIONS,
        MAX_VIDEO_BLOCKS, NO_BUS_ADDRESS, PAGE_FRAME, PAGE_UNTYPED, Region, STACK_REGION,
        VideoBlock,
    },
    libdevicetree::{
        DevTreeIndex, Node, dma_address, find_compatible,
        interrupts::{MAX_INTERRUPT_CELLS, interrupts},
        is_compatible, is_enabled, linked_phandle, node_by_phandle, phandle, regions,
    },
    libimage::{DriverImage, Link},
    libmeddle_runtime::ImportRecord,
    libobject::{
        CapError, FrameKey, IrqControlKey, IrqHandlerKey, KeySlot, KeyTableKey, ObjectType,
        PlacementWindow, RawKey, Rights, TimeKey, UntypedKey,
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

/// Most driver instances one boot loads.
pub const MAX_DRIVER_INSTANCES: usize = 8;

/// Pages of each driver's stack.
const STACK_PAGES: u32 = 4;

/// First slot of a driver's table Kickstart fills: the slots below are the
/// well-known ones every component table has.
const FIRST_DRIVER_SLOT: u32 = 5;

/// Guard of the first driver instance's table; later instances count up.
const FIRST_DRIVER_GUARD: u32 = 0x0D_0000;

/// The boot keys driver loading draws on.
#[derive(Clone, Copy)]
pub struct BootKeys {
    pub self_table: RawKey,
    pub untyped: RawKey,
    pub boot_time: RawKey,
    pub root_scheduler: RawKey,
    pub irq_control: RawKey,
}

impl BootKeys {
    pub fn of(boot: &BootState) -> Self {
        Self {
            self_table: boot.self_table_key,
            untyped: boot.boot_untyped_key,
            boot_time: boot.boot_time_key,
            root_scheduler: boot.root_scheduler_key,
            irq_control: boot.irq_control_key,
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
    node: Node<'static, 'static, 'static>,
    regions: [Region; MAX_REGIONS],
    region_count: usize,
    pages: [u64; MAX_DEVICE_PAGES],
    page_count: usize,
}

impl Instance {
    fn pages(&self) -> &[u64] {
        &self.pages[..self.page_count]
    }
}

/// The physical pages `node`'s registers span, sorted and deduplicated, with
/// its register ranges; `None` if they do not fit the init record.
fn plan_instance(
    driver: &'static DriverImage,
    compatible_index: usize,
    node: Node<'static, 'static, 'static>,
) -> Option<Instance> {
    let mut instance = Instance {
        driver,
        compatible_index,
        node,
        regions: [Region::default(); MAX_REGIONS],
        region_count: 0,
        pages: [0; MAX_DEVICE_PAGES],
        page_count: 0,
    };
    for region in regions(&instance.node) {
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
        let Some(instance) = plan_instance(driver, compatible_index, node.clone()) else {
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
fn grant(component: &Component, entry: KeyEntry, slot: u32) -> RawKey {
    // SAFETY: the component's retained table, borrowed for this bootstrap
    // grant only; no capability invocation overlaps it.
    unsafe { &mut *(component.table_addr as *mut KeyTable) }
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
    unsafe { &mut *(builder.boot_table_addr as *mut KeyTable) }
        .insert(KeySlot(slot), entry, BOOT_TABLE_GUARD)
        .unwrap_or_else(|failure| panic!("🥾 boot install failed: {:?}", failure.error.code()))
}

/// The node `links` lead to from `node`: a property step reads the first cell
/// of the current node's property as the phandle of the next node, and a
/// `compatible` first step starts from the one node with that string.
fn follow_links<'a, 'i, 'dt>(
    tree: &'a DevTreeIndex<'i, 'dt>,
    node: &Node<'a, 'i, 'dt>,
    links: &[Link],
) -> Option<Node<'a, 'i, 'dt>> {
    let mut current = node.clone();
    for (index, link) in links.iter().enumerate() {
        current = match *link {
            Link::Property(property) => node_by_phandle(tree, linked_phandle(&current, property)?)?,
            Link::Compatible(compatible) if index == 0 => find_compatible(tree, &[compatible])?.0,
            Link::Compatible(_) => return None,
        };
    }
    Some(current)
}

/// Whether two views of the device tree name the same node.
fn same_node(first: &Node<'_, '_, '_>, second: &Node<'_, '_, '_>) -> bool {
    first.name().ok() == second.name().ok() && phandle(first) == phandle(second)
}

/// A link chain for messages: the steps separated by commas.
struct Links(&'static [Link]);

impl core::fmt::Display for Links {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (index, link) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(", ")?;
            }
            match link {
                Link::Property(property) => formatter.write_str(property)?,
                Link::Compatible(compatible) => write!(formatter, "compatible \"{compatible}\"")?,
            }
        }
        Ok(())
    }
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
                0,
            )
            .unwrap_or_else(|error| panic!("🥾 data area Frame.Map failed: {:?}", error.code()));
    }
}

/// Pages of each target stack an exporting driver reserves for one client.
const EXPORT_STACK_PAGES: u32 = 4;

/// Minimum headroom of every exported function's target stack.
const EXPORT_STACK_HEADROOM: u64 = 0x400;

/// An instance loaded but not started: what wiring its imports and starting
/// it need.
struct Prepared<'p> {
    instance: &'p Instance,
    component: Component,
    /// The driver's guarded stack span, which also holds the target stacks
    /// of the functions it exports
    stacks: StackRegion,
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
        let name = instance.node.name().unwrap_or("?");
        let guard = FIRST_DRIVER_GUARD + u32::try_from(number).unwrap_or(0);
        let component = builder.component(nucleus, guard, slots);
        builder.load_component(&component, image, guard ^ 0x80_0000, slots);
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
        init.memory_bus = dma_address(&instance.node, memory_paddr, 1 << driver.memory_bits)
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
                .transfer(&TimeKey::from_key(time), driver.budget_ns, driver.priority)
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
        for ((record, &entry), &stack_top) in further_threads
            .iter_mut()
            .zip(driver.threads)
            .zip(&further_stack_tops)
        {
            *record = Some(configure_thread(entry, stack_top).0);
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
        for interrupt in interrupts(tree, &instance.node).take(MAX_INTERRUPTS) {
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
            let bits = 1_u16 << interrupt_count;
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
                bits: u64::from(bits),
                line: u64::from(issued.line),
            };
            interrupt_count += 1;
        }
        init.interrupt_count = interrupt_count as u64;
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
            init_page,
            thread,
            further_threads,
            next_slot,
        });
    }
    let prepared = &mut prepared[..count];

    // ── Imports ─────────────────────────────────────────────────────────
    let tree = device_tree::at_el1().index();
    // Target stack each (client, exporter) pair calls on, by instance index.
    let mut call_stacks = [[None::<UserStack>; MAX_DRIVER_INSTANCES]; MAX_DRIVER_INSTANCES];
    // The data area of each pair whose interface has one, as `(base, len)`,
    // the next free address of the window they share, and which instances
    // already have its page table installed.
    let mut call_areas = [[None::<(u64, u64)>; MAX_DRIVER_INSTANCES]; MAX_DRIVER_INSTANCES];
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
        let client_name = client_instance.node.name().unwrap_or("?");
        // Imports of one interface are consecutive: report a missing link once.
        let mut unlinked: Option<&str> = None;
        for import in client_image.imports {
            let target = follow_links(tree, &client_instance.node, import.links);
            let exporter = prepared.iter().position(|candidate| {
                candidate.as_ref().is_some_and(|candidate| {
                    target
                        .as_ref()
                        .is_some_and(|target| same_node(&candidate.instance.node, target))
                        && candidate
                            .instance
                            .driver
                            .component
                            .exports_interface(import.interface, import.type_code)
                })
            });
            let Some(exporter) = exporter else {
                if unlinked == Some(import.interface) {
                    continue;
                }
                unlinked = Some(import.interface);
                semi::println!(
                    "🥾 Driver {}: {client_name} links no instance exporting {} through `{}`",
                    client_image.name,
                    import.interface,
                    Links(import.links)
                );
                continue;
            };
            let stack = match call_stacks[client][exporter] {
                Some(stack) => stack,
                None => {
                    let exporting = prepared[exporter].as_mut().expect("a loaded exporter");
                    let stack =
                        builder.user_stack(&mut exporting.stacks, EXPORT_STACK_PAGES, slots);
                    call_stacks[client][exporter] = Some(stack);
                    stack
                }
            };
            let area = if import.data_bytes == 0 {
                None
            } else if let Some(area) = call_areas[client][exporter] {
                Some(area)
            } else {
                let bytes = import.data_bytes;
                let base = data_cursor;
                data_cursor += bytes;
                assert!(
                    data_cursor <= DATA_WINDOW_VA + DATA_WINDOW_BYTES,
                    "🥾 the data window is full"
                );
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
                        stack_bottom: stack.bottom,
                        stack_end: stack.top,
                        base,
                        len: bytes,
                    };
                    init.area_count += 1;
                });
                call_areas[client][exporter] = Some((base, bytes));
                Some((base, bytes))
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
            let (area_base, area_len) = area.unwrap_or((0, 0));
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
            let invocation = exporter_space
                .create_invocation_with_window(
                    entry,
                    &calling.component.table(),
                    KeySlot(slot),
                    stack.bottom,
                    stack.top,
                    EXPORT_STACK_HEADROOM,
                    window.unwrap_or(PlacementWindow::NONE),
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "🥾 CreateInvocation `{}` failed: {:?}",
                        import.export,
                        error.code()
                    )
                });
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
                    stack_end: stack.top,
                    area_base,
                    area_len,
                };
                init.import_count += 1;
            });
        }
    }

    // ── Start ───────────────────────────────────────────────────────────
    let mut started = 0;
    for loaded in prepared.iter_mut().flatten() {
        let instance = loaded.instance;
        let name = instance.node.name().unwrap_or("?");
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
        before_start(&LoadedDriver {
            driver: instance.driver,
            node: name,
            component: &loaded.component,
            thread: loaded.thread,
            init: &init,
            first_free_slot,
        });
        for thread in
            core::iter::once(loaded.thread).chain(loaded.further_threads.into_iter().flatten())
        {
            root_scheduler
                .start_thread(thread)
                .unwrap_or_else(|error| panic!("🥾 driver StartThread failed: {:?}", error.code()));
        }
        started += 1;
    }
    started
}
