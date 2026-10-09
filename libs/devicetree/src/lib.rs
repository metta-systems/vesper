#![no_std]

//! Device tree helpers on top of the `fdt-rs` index.
//!
//! - cell sizes follow `DTSpec` v0.3 §2.3.5: a node's `reg` is encoded with the
//!   `#address-cells`/`#size-cells` of its **parent** (defaults 2 and 1);
//! - `reg` addresses are bus addresses, translated to CPU physical addresses
//!   through every ancestor bus's `ranges` (§2.3.8);
//! - `compatible` is a string list matched entry by entry.

mod dump;
mod tree;

pub use {
    dump::dump,
    tree::{Device, DeviceTree},
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
const DEFAULT_ADDRESS_CELLS: u32 = 2;
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
fn read_number(prop: &Prop<'_, '_, '_>, index: usize, count: u32) -> Option<u64> {
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

/// `node`'s `phandle`, if it has one.
pub fn phandle(node: &Node<'_, '_, '_>) -> Option<u32> {
    property(node, "phandle").and_then(|prop| prop.u32(0).ok())
}

/// The node in `tree` whose `phandle` property equals `phandle`.
pub fn node_by_phandle<'a, 'i: 'a, 'dt: 'i>(
    tree: &'a DevTreeIndex<'i, 'dt>,
    phandle: u32,
) -> Option<Node<'a, 'i, 'dt>> {
    tree.nodes()
        .find(|node| self::phandle(node) == Some(phandle))
}
