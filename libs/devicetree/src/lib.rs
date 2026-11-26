#![no_std]

//! Device tree helpers on top of the `fdt-rs` index.
//!
//! - cell sizes follow `DTSpec` v0.3 §2.3.5: a node's `reg` is encoded with the
//!   `#address-cells`/`#size-cells` of its **parent** (defaults 2 and 1);
//! - `reg` addresses are bus addresses, translated to CPU physical addresses
//!   through every ancestor bus's `ranges` (§2.3.8);
//! - `compatible` is a string list matched entry by entry;
//! - [`interrupts`] resolves interrupt routing: the generic timer, the root
//!   controller and each node's interrupts with their controller;
//! - [`cpus`] lists the CPUs with their affinity and start method, and the PSCI node;
//! - [`DeviceTree::board`] and its siblings find what boot programs need
//!   before any driver runs: the board, the console UART, the firmware mailbox
//!   and the activity LED.

mod board;
pub mod cpus;
mod dump;
pub mod interrupts;
pub mod pci;
mod tree;

pub use {
    board::{
        ActivityLed, Board, BootHardware, ConsoleUart, FIRMWARE_GPIO_BASE, Gpio, GpioKind, Mailbox,
        Soc, UartClock,
    },
    dump::dump,
    tree::{Device, DeviceTree, INDEX_BUFFER_BYTES},
};

pub use fdt_rs::{
    base::DevTree,
    error::DevTreeError,
    index::{DevTreeIndex, DevTreeIndexNode, DevTreeIndexProp},
    prelude::{FallibleIterator, PropReader},
};

/// An indexed device tree node.
pub type Node<'a, 'i, 'dt> = DevTreeIndexNode<'a, 'i, 'dt>;
/// An indexed device tree property.
pub type Prop<'a, 'i, 'dt> = DevTreeIndexProp<'a, 'i, 'dt>;

/// Default `#address-cells` when a bus does not specify it (`DTSpec` §2.3.5).
pub(crate) const DEFAULT_ADDRESS_CELLS: u32 = 2;
/// Default `#size-cells` when a bus does not specify it (`DTSpec` §2.3.5).
const DEFAULT_SIZE_CELLS: u32 = 1;

/// Cell counts a bus node declares for the addresses and sizes of its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cells {
    pub address: u32,
    pub size: u32,
}

/// An address range: start and size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub start: u64,
    pub size: u64,
}

/// Find a property of `node` by name.
pub fn property<'a, 'i: 'a, 'dt: 'i>(
    node: &Node<'a, 'i, 'dt>,
    name: &str,
) -> Option<Prop<'a, 'i, 'dt>> {
    node.props().find(|prop| prop.name() == Ok(name))
}

/// Cell counts `bus` declares for its children's `reg` and `ranges` entries.
pub fn bus_cells(bus: &Node<'_, '_, '_>) -> Cells {
    let read = |name, default| {
        property(bus, name)
            .and_then(|prop| prop.u32(0).ok())
            .unwrap_or(default)
    };
    Cells {
        address: read("#address-cells", DEFAULT_ADDRESS_CELLS),
        size: read("#size-cells", DEFAULT_SIZE_CELLS),
    }
}

/// Cell counts used to encode `node`'s own `reg`: those of its parent bus.
pub fn reg_cells(node: &Node<'_, '_, '_>) -> Cells {
    node.parent()
        .map_or_else(|| bus_cells(node), |parent| bus_cells(&parent))
}

/// Read a `count`-cell big-endian number starting at u32 `index` of `prop`.
pub(crate) fn read_number(prop: &Prop<'_, '_, '_>, index: usize, count: u32) -> Option<u64> {
    match count {
        0 => Some(0),
        1 => prop.u32(index).ok().map(u64::from),
        2 => {
            let high = u64::from(prop.u32(index).ok()?);
            let low = u64::from(prop.u32(index + 1).ok()?);
            Some(high << 32 | low)
        }
        // Wider numbers (e.g. PCI 3-cell addresses) do not fit a CPU address.
        _ => None,
    }
}

/// Iterator over `(address, size)` entries of a `reg`-style property.
pub struct RegIter<'a, 'i: 'a, 'dt: 'i> {
    prop: Option<Prop<'a, 'i, 'dt>>,
    cells: Cells,
    index: usize,
}

impl Iterator for RegIter<'_, '_, '_> {
    type Item = Region;

    fn next(&mut self) -> Option<Region> {
        let prop = self.prop.as_ref()?;
        let stride = (self.cells.address + self.cells.size) as usize;
        if stride == 0 || (self.index + stride) * size_of::<u32>() > prop.length() {
            return None;
        }
        let start = read_number(prop, self.index, self.cells.address)?;
        let size = read_number(
            prop,
            self.index + self.cells.address as usize,
            self.cells.size,
        )?;
        self.index += stride;
        Some(Region { start, size })
    }
}

/// The untranslated (bus address) entries of `node`'s `reg` property.
pub fn reg<'a, 'i: 'a, 'dt: 'i>(node: &Node<'a, 'i, 'dt>) -> RegIter<'a, 'i, 'dt> {
    RegIter {
        prop: property(node, "reg"),
        cells: reg_cells(node),
        index: 0,
    }
}

/// Translate a bus `address` from `node`'s `reg` into a CPU physical address.
///
/// Walks up through every ancestor bus: an empty `ranges` is an identity map,
/// a missing `ranges` means the bus is not memory-mapped (returns `None`),
/// otherwise the matching `(child, parent, size)` entry rebases the address.
pub fn translate(node: &Node<'_, '_, '_>, address: u64) -> Option<u64> {
    let mut address = address;
    let mut bus = node.parent()?;
    while let Some(grandparent) = bus.parent() {
        let ranges = property(&bus, "ranges")?;
        if ranges.length() != 0 {
            let child_cells = bus_cells(&bus);
            let parent_address_cells = bus_cells(&grandparent).address;
            let stride = (child_cells.address + parent_address_cells + child_cells.size) as usize;
            let entries = ranges.length() / size_of::<u32>() / stride;
            address = (0..entries).find_map(|entry| {
                let index = entry * stride;
                let child_base = read_number(&ranges, index, child_cells.address)?;
                let parent_base = read_number(
                    &ranges,
                    index + child_cells.address as usize,
                    parent_address_cells,
                )?;
                let size = read_number(
                    &ranges,
                    index + (child_cells.address + parent_address_cells) as usize,
                    child_cells.size,
                )?;
                (child_base..child_base.checked_add(size)?)
                    .contains(&address)
                    .then(|| parent_base + (address - child_base))
            })?;
        }
        bus = grandparent;
    }
    Some(address)
}

/// The address of `count` cells at `index`, as [`read_number`] reads it; a PCI
/// address (three cells: the space code, then the 64-bit address) is its
/// address cells, the space code dropped.
fn read_bus_address(prop: &Prop<'_, '_, '_>, index: usize, count: u32) -> Option<u64> {
    match count {
        3 => read_number(prop, index + 1, 2),
        _ => read_number(prop, index, count),
    }
}

/// Most bus levels [`dma_address`] walks above a node.
const MAX_BUS_DEPTH: usize = 8;

/// The address at which devices under `node`'s parent bus reach the CPU
/// physical range `[cpu_address, cpu_address + size)`, or `None` if a bus on
/// the way does not map all of it.
///
/// Applies every ancestor bus's `dma-ranges` from the root down; a missing or
/// empty `dma-ranges` maps addresses one to one. Each matching
/// `(child, parent, size)` entry rebases a parent address into the bus.
pub fn dma_address(node: &Node<'_, '_, '_>, cpu_address: u64, size: u64) -> Option<u64> {
    let mut buses = [const { None }; MAX_BUS_DEPTH];
    let mut depth = 0;
    let mut bus = node.parent()?;
    while let Some(grandparent) = bus.parent() {
        *buses.get_mut(depth)? = Some((bus, grandparent.clone()));
        depth += 1;
        bus = grandparent;
    }
    let mut address = cpu_address;
    for (bus, parent) in buses[..depth].iter().rev().flatten() {
        let Some(dma_ranges) = property(bus, "dma-ranges") else {
            continue;
        };
        if dma_ranges.length() == 0 {
            continue;
        }
        let child_cells = bus_cells(bus);
        let parent_address_cells = bus_cells(parent).address;
        let stride = (child_cells.address + parent_address_cells + child_cells.size) as usize;
        let entries = dma_ranges.length() / size_of::<u32>() / stride;
        address = (0..entries).find_map(|entry| {
            let index = entry * stride;
            let child_base = read_bus_address(&dma_ranges, index, child_cells.address)?;
            let parent_base = read_bus_address(
                &dma_ranges,
                index + child_cells.address as usize,
                parent_address_cells,
            )?;
            let length = read_number(
                &dma_ranges,
                index + (child_cells.address + parent_address_cells) as usize,
                child_cells.size,
            )?;
            let end = address.checked_add(size)?;
            (address >= parent_base && end <= parent_base.checked_add(length)?)
                .then(|| child_base + (address - parent_base))
        })?;
    }
    Some(address)
}

/// Iterator over a node's `reg` entries translated to CPU physical addresses;
/// entries that cannot be translated are skipped.
pub struct Regions<'a, 'i: 'a, 'dt: 'i> {
    node: Node<'a, 'i, 'dt>,
    entries: RegIter<'a, 'i, 'dt>,
}

impl Iterator for Regions<'_, '_, '_> {
    type Item = Region;

    fn next(&mut self) -> Option<Region> {
        self.entries.by_ref().find_map(|region| {
            translate(&self.node, region.start).map(|start| Region {
                start,
                size: region.size,
            })
        })
    }
}

/// `node`'s `reg` entries translated to CPU physical addresses.
pub fn regions<'a, 'i: 'a, 'dt: 'i>(node: &Node<'a, 'i, 'dt>) -> Regions<'a, 'i, 'dt> {
    Regions {
        node: node.clone(),
        entries: reg(node),
    }
}

/// Whether `node`'s `compatible` string list contains `compatible`.
pub fn is_compatible(node: &Node<'_, '_, '_>, compatible: &str) -> bool {
    property(node, "compatible").is_some_and(|prop| {
        prop.iter_str()
            .any(|entry| Ok(entry == compatible))
            .unwrap_or(false)
    })
}

/// The first node in `tree` compatible with any of `compatibles`, with the matched string.
pub fn find_compatible<'a, 'i: 'a, 'dt: 'i, 's>(
    tree: &'a DevTreeIndex<'i, 'dt>,
    compatibles: &[&'s str],
) -> Option<(Node<'a, 'i, 'dt>, &'s str)> {
    tree.nodes().find_map(|node| {
        compatibles
            .iter()
            .find(|compatible| is_compatible(&node, compatible))
            .map(|compatible| (node.clone(), *compatible))
    })
}

/// Most levels [`write_path`] walks up from a node.
const MAX_PATH_DEPTH: usize = 16;

/// Write `node`'s full path (`/soc/spi@7e204000/panel@0`, `/` for the root)
/// into `out`; returns its length, or `None` when it does not fit `out` or
/// the node sits deeper than 16 levels.
pub fn write_path(node: &Node<'_, '_, '_>, out: &mut [u8]) -> Option<usize> {
    let mut names = [""; MAX_PATH_DEPTH];
    let mut depth = 0;
    let mut current = Some(node.clone());
    while let Some(level) = current {
        let parent = level.parent();
        // The root has no parent; its name is empty.
        if parent.is_some() {
            *names.get_mut(depth)? = level.name().ok()?;
            depth += 1;
        }
        current = parent;
    }
    if depth == 0 {
        *out.first_mut()? = b'/';
        return Some(1);
    }
    let mut len = 0;
    for name in names[..depth].iter().rev() {
        let end = len + 1 + name.len();
        let part = out.get_mut(len..end)?;
        part[0] = b'/';
        part[1..].copy_from_slice(name.as_bytes());
        len = end;
    }
    Some(len)
}

/// Whether `node` is enabled: its `status` is absent, `"okay"` or `"ok"`.
pub fn is_enabled(node: &Node<'_, '_, '_>) -> bool {
    property(node, "status")
        .and_then(|prop| prop.str().ok())
        .is_none_or(|status| status == "okay" || status == "ok")
}

/// The `index`-th interrupt specifier of `node`'s `interrupts` property,
/// `cell_count` cells wide (the controller's `#interrupt-cells`).
pub fn interrupt_specifier<'n>(
    node: &Node<'_, '_, '_>,
    index: usize,
    cell_count: usize,
    cells: &'n mut [u32],
) -> Option<&'n [u32]> {
    let prop = property(node, "interrupts")?;
    let cells = cells.get_mut(..cell_count)?;
    for (position, cell) in cells.iter_mut().enumerate() {
        *cell = prop.u32(index * cell_count + position).ok()?;
    }
    Some(cells)
}

/// A controller's `#interrupt-cells`.
pub fn interrupt_cells(controller: &Node<'_, '_, '_>) -> Option<u32> {
    property(controller, "#interrupt-cells").and_then(|prop| prop.u32(0).ok())
}

/// Whether `node` carries the `interrupt-controller` marker property.
pub fn is_interrupt_controller(node: &Node<'_, '_, '_>) -> bool {
    property(node, "interrupt-controller").is_some()
}

/// The interrupt controller `node`'s `interrupts` are routed to.
///
/// `interrupt-parent` is inherited: the nearest ancestor (or the node itself)
/// that sets it names the controller by phandle (`DTSpec` §2.4.1).
pub fn interrupt_parent<'a, 'i: 'a, 'dt: 'i>(
    tree: &'a DevTreeIndex<'i, 'dt>,
    node: &Node<'a, 'i, 'dt>,
) -> Option<Node<'a, 'i, 'dt>> {
    let mut current = Some(node.clone());
    while let Some(candidate) = current {
        if let Some(phandle) =
            property(&candidate, "interrupt-parent").and_then(|prop| prop.u32(0).ok())
        {
            return node_by_phandle(tree, phandle);
        }
        current = candidate.parent();
    }
    None
}

/// Whether `node`'s full path is `path`, such as
/// `/scb/pcie@7d500000/pci@1,0/usb@1,0` (unit addresses included).
pub fn has_path(node: &Node<'_, '_, '_>, path: &str) -> bool {
    let mut current = Some(node.clone());
    // The root node's name is empty, as is the part before the first `/`.
    for component in path.rsplit('/') {
        match current {
            Some(candidate) if candidate.name() == Ok(component) => current = candidate.parent(),
            _ => return false,
        }
    }
    current.is_none()
}

/// `node`'s `phandle`, if it has one.
pub fn phandle(node: &Node<'_, '_, '_>) -> Option<u32> {
    property(node, "phandle").and_then(|prop| prop.u32(0).ok())
}

/// The phandle in the first cell of `node`'s `property`: the node it links
/// to, as in a consumer's `clocks = <&provider ...>` or `mboxes = <&mailbox>`.
pub fn linked_phandle(node: &Node<'_, '_, '_>, property_name: &str) -> Option<u32> {
    property(node, property_name).and_then(|prop| prop.u32(0).ok())
}

/// The frequency of `node`'s first clock (`clocks`), in Hz, when that clock is
/// a `fixed-clock` node; `None` for a clock another provider runs.
pub fn fixed_clock_rate(tree: &DevTreeIndex<'_, '_>, node: &Node<'_, '_, '_>) -> Option<u32> {
    let clock = node_by_phandle(tree, linked_phandle(node, "clocks")?)?;
    if !is_compatible(&clock, "fixed-clock") {
        return None;
    }
    property(&clock, "clock-frequency")?.u32(0).ok()
}

/// The DMA channels a Broadcom DMA controller node (`brcm,bcm2835-dma`)
/// leaves to the ARM, one bit per channel: its `brcm,dma-channel-mask`. The
/// firmware may change it at boot, so it is read from the tree it hands over.
pub fn brcm_dma_channel_mask(node: &Node<'_, '_, '_>) -> Option<u32> {
    property(node, "brcm,dma-channel-mask").and_then(|prop| prop.u32(0).ok())
}

/// The AMBA peripheral ID a `PrimeCell` node states in place of its ID
/// registers (`arm,primecell-periphid`): `PeriphID3` to `PeriphID0`, one byte
/// each, as one word. A node gives it when the registers are absent or wrong,
/// as the Raspberry Pi 5's `uart10`, whose 0x200-byte block ends before them.
pub fn primecell_periphid(node: &Node<'_, '_, '_>) -> Option<u32> {
    property(node, "arm,primecell-periphid").and_then(|prop| prop.u32(0).ok())
}

/// One pin of a Broadcom pin group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrcmPin {
    pub pin: u32,
    /// The code of the GPIO function select registers (0 input, 1 output, 4
    /// to 7 ALT0 to ALT3, 3 ALT4, 2 ALT5)
    pub function: u32,
    /// The pull code (0 off, 1 down, 2 up), if the group sets one
    pub pull: Option<u32>,
}

/// The pins `node`'s default pin state (`pinctrl-0`) sets up, from the
/// Broadcom pin controller's two bindings:
///
/// - each linked group's `brcm,pins`, with its `brcm,function` and optional
///   `brcm,pull`, each one value for every pin or one per pin (the BCM2835
///   trees);
/// - the generic pin configuration (the BCM2711 trees): the group node and
///   each of its children may name `pins` (`"gpio40"`, ...) with a `function`
///   (`"alt0"`, ...) and a `bias-disable`, `bias-pull-down` or `bias-pull-up`.
pub fn brcm_pins<'a, 'i: 'a, 'dt: 'i>(
    tree: &'a DevTreeIndex<'i, 'dt>,
    node: &Node<'a, 'i, 'dt>,
) -> impl Iterator<Item = BrcmPin> + 'a {
    let groups = property(node, "pinctrl-0");
    let group_count = groups
        .as_ref()
        .map_or(0, |prop| prop.length() / size_of::<u32>());
    (0..group_count)
        .filter_map(move |index| groups.as_ref()?.u32(index).ok())
        .filter_map(move |group| node_by_phandle(tree, group))
        .flat_map(|group| brcm_binding_pins(&group).chain(generic_pins(&group)))
}

/// The function select code a generic pin configuration's `function` names
/// (the Broadcom pin controller binding: `gpio_in`, `gpio_out`, `alt0` to
/// `alt5`).
#[must_use]
pub fn generic_function_code(function: &str) -> Option<u32> {
    match function {
        "gpio_in" => Some(0),
        "gpio_out" => Some(1),
        "alt0" => Some(4),
        "alt1" => Some(5),
        "alt2" => Some(6),
        "alt3" => Some(7),
        "alt4" => Some(3),
        "alt5" => Some(2),
        _ => None,
    }
}

/// The pins of `group` in the generic pin configuration: its own `pins` and
/// each child's.
fn generic_pins<'a, 'i: 'a, 'dt: 'i>(
    group: &Node<'a, 'i, 'dt>,
) -> impl Iterator<Item = BrcmPin> + use<'a, 'i, 'dt> {
    core::iter::once(group.clone())
        .chain(group.children())
        .flat_map(|config| {
            let function = property(&config, "function")
                .and_then(|prop| prop.str().ok())
                .and_then(generic_function_code);
            let pull = [
                ("bias-disable", 0),
                ("bias-pull-down", 1),
                ("bias-pull-up", 2),
            ]
            .into_iter()
            .find_map(|(name, code)| property(&config, name).map(|_| code));
            let pins = property(&config, "pins");
            let pin_count = pins
                .as_ref()
                .map_or(0, |prop| prop.iter_str().count().unwrap_or(0));
            (0..pin_count).filter_map(move |index| {
                let name = pins.as_ref()?.iter_str().nth(index).ok()??;
                Some(BrcmPin {
                    pin: name.strip_prefix("gpio")?.parse().ok()?,
                    function: function?,
                    pull,
                })
            })
        })
}

/// The pins of `group` in the `brcm,pins` binding.
fn brcm_binding_pins<'a, 'i: 'a, 'dt: 'i>(
    group: &Node<'a, 'i, 'dt>,
) -> impl Iterator<Item = BrcmPin> + use<'a, 'i, 'dt> {
    let pins = property(group, "brcm,pins");
    let functions = property(group, "brcm,function");
    let pulls = property(group, "brcm,pull");
    let pin_count = pins
        .as_ref()
        .map_or(0, |prop| prop.length() / size_of::<u32>());
    // A per-pin property with a single value applies it to every pin.
    let value_of = |prop: &Option<Prop<'a, 'i, 'dt>>, index: usize| {
        let prop = prop.as_ref()?;
        let shared = prop.length() == size_of::<u32>();
        prop.u32(if shared { 0 } else { index }).ok()
    };
    (0..pin_count).filter_map(move |index| {
        Some(BrcmPin {
            pin: pins.as_ref()?.u32(index).ok()?,
            function: value_of(&functions, index)?,
            pull: value_of(&pulls, index),
        })
    })
}

/// The GPIO pins of the controller `controller` (by its phandle) that `node`
/// uses, as a mask (bit `n` is pin `n`; pins above 62 are left out):
///
/// - the pins of its default pin state (`pinctrl-0`, see [`brcm_pins`]);
/// - the pin of every specifier of its `gpios` and `<name>-gpios` properties
///   that names `controller` (`reset-gpios = <&gpio 25 0>`), each specifier
///   one phandle and the controller's `#gpio-cells` (2 when absent) cells, the
///   first the pin;
/// - the same of the node its `backlight` property links to
///   (`gpio-backlight`, whose `gpios` name the LED pin).
pub fn gpio_pins(tree: &DevTreeIndex<'_, '_>, node: &Node<'_, '_, '_>, controller: u32) -> u64 {
    let mut mask = brcm_pins(tree, node).fold(0, |mask, pin| with_pin(mask, pin.pin));
    mask |= gpio_specifier_pins(tree, node, controller);
    if let Some(backlight) =
        linked_phandle(node, "backlight").and_then(|linked| node_by_phandle(tree, linked))
    {
        mask |= gpio_specifier_pins(tree, &backlight, controller);
    }
    mask
}

/// `mask` with `pin` added, if it fits.
const fn with_pin(mask: u64, pin: u32) -> u64 {
    if pin < 63 { mask | (1 << pin) } else { mask }
}

/// The pins of `controller` that `node`'s `gpios` and `<name>-gpios`
/// specifiers name.
fn gpio_specifier_pins(
    tree: &DevTreeIndex<'_, '_>,
    node: &Node<'_, '_, '_>,
    controller: u32,
) -> u64 {
    let mut mask = 0;
    for prop in node.props() {
        let Ok(name) = prop.name() else { continue };
        if name != "gpios" && !name.ends_with("-gpios") {
            continue;
        }
        let words = prop.length() / size_of::<u32>();
        let mut at = 0;
        while at < words {
            let Ok(target) = prop.u32(at) else { break };
            let cells = node_by_phandle(tree, target)
                .and_then(|provider| property(&provider, "#gpio-cells"))
                .and_then(|cells| cells.u32(0).ok())
                .map_or(2, |cells| cells as usize);
            if target == controller
                && let Ok(pin) = prop.u32(at + 1)
            {
                mask = with_pin(mask, pin);
            }
            at += 1 + cells;
        }
    }
    mask
}

/// One GPIO a `<name>-gpios` property names: the controller's phandle, the
/// pin, and the flags cell (bit 0: active low) when the controller has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpioSpecifier {
    pub controller: u32,
    pub pin: u32,
    pub flags: u32,
}

impl GpioSpecifier {
    /// Whether the line is asserted by driving it low (flags bit 0).
    #[must_use]
    pub const fn active_low(&self) -> bool {
        self.flags & 1 != 0
    }
}

/// The `index`-th specifier of `node`'s `property` (`cs-gpios`, `dc-gpios`,
/// ...), each one phandle and the controller's `#gpio-cells` (2 when absent)
/// cells: the pin, then the flags.
pub fn gpio_specifier(
    tree: &DevTreeIndex<'_, '_>,
    node: &Node<'_, '_, '_>,
    property_name: &str,
    index: usize,
) -> Option<GpioSpecifier> {
    let prop = property(node, property_name)?;
    let words = prop.length() / size_of::<u32>();
    let mut at = 0;
    let mut position = 0;
    while at < words {
        let controller = prop.u32(at).ok()?;
        let cells = node_by_phandle(tree, controller)
            .and_then(|provider| property(&provider, "#gpio-cells"))
            .and_then(|cells| cells.u32(0).ok())
            .map_or(2, |cells| cells as usize);
        if position == index {
            return Some(GpioSpecifier {
                controller,
                pin: prop.u32(at + 1).ok()?,
                flags: if cells >= 2 {
                    prop.u32(at + 2).ok()?
                } else {
                    0
                },
            });
        }
        at += 1 + cells;
        position += 1;
    }
    None
}

/// The node in `tree` whose `phandle` property equals `phandle`.
pub fn node_by_phandle<'a, 'i: 'a, 'dt: 'i>(
    tree: &'a DevTreeIndex<'i, 'dt>,
    phandle: u32,
) -> Option<Node<'a, 'i, 'dt>> {
    tree.nodes()
        .find(|node| self::phandle(node) == Some(phandle))
}
