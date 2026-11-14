//! PCI host bridge nodes (`device_type = "pci"`, the IEEE 1275 PCI bus
//! binding and the Linux `pci-bus-common` schema): their outbound `ranges`,
//! inbound `dma-ranges`, MSI controller and resets.
//!
//! A PCI address is three cells: `phys.hi` (`npt000ss bbbbbbbb dddddfff
//! rrrrrrrr`: relocatable, prefetchable, aliased, the space code, then bus,
//! device, function and register) and a 64-bit address in `phys.mid` and
//! `phys.lo`.

use crate::{
    DevTreeIndex, FallibleIterator, Node, Prop, PropReader, bus_cells, interrupt_cells,
    node_by_phandle, property, read_number, translate,
};

/// Cells of a PCI address.
const PCI_ADDRESS_CELLS: usize = 3;

/// `phys.hi` bit 30: the range is prefetchable.
const PHYS_HI_PREFETCHABLE: u32 = 1 << 30;
/// `phys.hi` bits 25:24: the space code.
const PHYS_HI_SPACE_SHIFT: u32 = 24;
const PHYS_HI_SPACE_MASK: u32 = 0b11;

/// The address space a PCI range is in: `phys.hi`'s space code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciSpace {
    Configuration,
    Io,
    /// 32-bit memory
    Memory32,
    /// 64-bit memory
    Memory64,
}

impl PciSpace {
    /// The space a `phys.hi` cell names.
    pub const fn from_phys_hi(phys_hi: u32) -> Self {
        match (phys_hi >> PHYS_HI_SPACE_SHIFT) & PHYS_HI_SPACE_MASK {
            0 => Self::Configuration,
            1 => Self::Io,
            2 => Self::Memory32,
            _ => Self::Memory64,
        }
    }
}

/// One entry of a host bridge's `ranges` (outbound: CPU addresses that reach
/// PCI) or `dma-ranges` (inbound: PCI addresses that reach the CPU's side).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciRange {
    pub space: PciSpace,
    pub prefetchable: bool,
    /// The address on the PCI side
    pub pci: u64,
    /// The CPU physical address: for `ranges` translated through every
    /// ancestor's `ranges`, for `dma-ranges` through every ancestor's
    /// `dma-ranges`
    pub cpu: u64,
    pub size: u64,
}

/// The entries of `node`'s PCI-to-parent property `name` (`ranges` or
/// `dma-ranges`), parent addresses untranslated.
fn raw_ranges<'a, 'i: 'a, 'dt: 'i>(
    node: &Node<'a, 'i, 'dt>,
    name: &str,
) -> impl Iterator<Item = PciRange> + 'a {
    let prop = property(node, name);
    let parent_cells = node
        .parent()
        .map_or(crate::DEFAULT_ADDRESS_CELLS, |parent| bus_cells(&parent).address);
    let size_cells = bus_cells(node).size;
    let stride = PCI_ADDRESS_CELLS + parent_cells as usize + size_cells as usize;
    let entries = prop
        .as_ref()
        .map_or(0, |prop| prop.length() / size_of::<u32>() / stride);
    (0..entries).filter_map(move |entry| {
        let prop: &Prop<'_, '_, '_> = prop.as_ref()?;
        let index = entry * stride;
        let phys_hi = prop.u32(index).ok()?;
        Some(PciRange {
            space: PciSpace::from_phys_hi(phys_hi),
            prefetchable: phys_hi & PHYS_HI_PREFETCHABLE != 0,
            pci: read_number(prop, index + 1, 2)?,
            cpu: read_number(prop, index + PCI_ADDRESS_CELLS, parent_cells)?,
            size: read_number(
                prop,
                index + PCI_ADDRESS_CELLS + parent_cells as usize,
                size_cells,
            )?,
        })
    })
}

/// Whether `node` is a PCI host bridge: a `device_type = "pci"` node whose
/// parent is no PCI bus (a PCI-to-PCI bridge below it is not one).
pub fn is_host_bridge(node: &Node<'_, '_, '_>) -> bool {
    let is_pci = |node: &Node<'_, '_, '_>| {
        property(node, "device_type")
            .and_then(|prop| prop.str().ok())
            .is_some_and(|kind| kind == "pci")
    };
    is_pci(node) && !node.parent().is_some_and(|parent| is_pci(&parent))
}

/// The outbound windows of host bridge `node`: its `ranges`, with CPU
/// addresses; an entry no ancestor bus maps is skipped.
pub fn pci_ranges<'a, 'i: 'a, 'dt: 'i>(
    node: &Node<'a, 'i, 'dt>,
) -> impl Iterator<Item = PciRange> + 'a {
    let bridge = node.clone();
    raw_ranges(node, "ranges").filter_map(move |range| {
        Some(PciRange {
            cpu: translate(&bridge, range.cpu)?,
            ..range
        })
    })
}

/// Rebase `address` on `bus`'s parent through `bus`'s `dma-ranges`: an absent
/// or empty `dma-ranges` maps one to one.
fn dma_to_parent(bus: &Node<'_, '_, '_>, address: u64) -> Option<u64> {
    let Some(dma_ranges) = property(bus, "dma-ranges") else {
        return Some(address);
    };
    if dma_ranges.length() == 0 {
        return Some(address);
    }
    let child_cells = bus_cells(bus);
    let parent_address_cells = bus
        .parent()
        .map_or(crate::DEFAULT_ADDRESS_CELLS, |parent| bus_cells(&parent).address);
    let stride = (child_cells.address + parent_address_cells + child_cells.size) as usize;
    let entries = dma_ranges.length() / size_of::<u32>() / stride;
    (0..entries).find_map(|entry| {
        let index = entry * stride;
        let child_base = read_number(&dma_ranges, index, child_cells.address)?;
        let parent_base = read_number(
            &dma_ranges,
            index + child_cells.address as usize,
            parent_address_cells,
        )?;
        let length = read_number(
            &dma_ranges,
            index + (child_cells.address + parent_address_cells) as usize,
            child_cells.size,
        )?;
        (child_base..child_base.checked_add(length)?)
            .contains(&address)
            .then(|| parent_base + (address - child_base))
    })
}

/// The inbound windows of host bridge `node`: its `dma-ranges`, with the
/// CPU physical address each PCI range reaches; an entry an ancestor bus
/// does not map is skipped.
pub fn pci_dma_ranges<'a, 'i: 'a, 'dt: 'i>(
    node: &Node<'a, 'i, 'dt>,
) -> impl Iterator<Item = PciRange> + 'a {
    let bridge = node.clone();
    raw_ranges(node, "dma-ranges").filter_map(move |range| {
        let mut address = range.cpu;
        let mut bus = bridge.parent()?;
        while let Some(parent) = bus.parent() {
            address = dma_to_parent(&bus, address)?;
            bus = parent;
        }
        Some(PciRange {
            cpu: address,
            ..range
        })
    })
}

/// `node`'s MSI controller (`msi-parent`); the node itself when it serves
/// its own MSIs.
pub fn msi_parent<'a, 'i: 'a, 'dt: 'i>(
    tree: &'a DevTreeIndex<'i, 'dt>,
    node: &Node<'a, 'i, 'dt>,
) -> Option<Node<'a, 'i, 'dt>> {
    let phandle = property(node, "msi-parent")?.u32(0).ok()?;
    node_by_phandle(tree, phandle)
}

/// Most cells of an `msi-ranges` interrupt specifier this crate decodes.
pub const MAX_MSI_CELLS: usize = 4;

/// The first entry of an MSI controller's `msi-ranges`: the interrupt its
/// first vector raises and how many consecutive interrupts its vectors
/// raise (the `msi-controller` binding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MsiRange {
    /// The phandle of the interrupt controller the vectors raise lines of
    pub controller: u32,
    pub cell_count: u32,
    /// The first vector's interrupt specifier
    pub cells: [u32; MAX_MSI_CELLS],
    pub count: u32,
}

impl MsiRange {
    /// The first vector's interrupt specifier.
    pub fn cells(&self) -> &[u32] {
        &self.cells[..(self.cell_count as usize).min(MAX_MSI_CELLS)]
    }
}

/// The first entry of MSI controller `controller`'s `msi-ranges`.
pub fn msi_range(tree: &DevTreeIndex<'_, '_>, controller: &Node<'_, '_, '_>) -> Option<MsiRange> {
    let prop = property(controller, "msi-ranges")?;
    let parent = prop.u32(0).ok()?;
    let cell_count = interrupt_cells(&node_by_phandle(tree, parent)?)?;
    let mut cells = [0; MAX_MSI_CELLS];
    for (index, cell) in cells.iter_mut().take(cell_count as usize).enumerate() {
        *cell = prop.u32(1 + index).ok()?;
    }
    if cell_count as usize > MAX_MSI_CELLS {
        return None;
    }
    Some(MsiRange {
        controller: parent,
        cell_count,
        cells,
        count: prop.u32(1 + cell_count as usize).ok()?,
    })
}

/// The vector number an MSI controller's first `msi-ranges` interrupt
/// stands for: its `brcm,msi-offset` (the `brcm,bcm2712-mip` binding), zero
/// without one.
pub fn brcm_msi_offset(controller: &Node<'_, '_, '_>) -> u32 {
    property(controller, "brcm,msi-offset")
        .and_then(|prop| prop.u32(0).ok())
        .unwrap_or(0)
}

/// One entry of a node's `resets` (the reset binding): the reset controller
/// and the reset's specifier, with its `reset-names` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reset<'dt> {
    /// The controller's phandle
    pub controller: u32,
    /// The specifier's one cell; zero for a controller with
    /// `#reset-cells = <0>`
    pub id: u32,
    /// Its `reset-names` entry, empty without one
    pub name: &'dt str,
}

/// `node`'s resets, in order. An entry whose controller is missing, or
/// declares more than one reset cell, ends the list.
pub fn resets<'a, 'i: 'a, 'dt: 'i>(
    tree: &'a DevTreeIndex<'i, 'dt>,
    node: &Node<'a, 'i, 'dt>,
) -> impl Iterator<Item = Reset<'dt>> + 'a {
    let prop = property(node, "resets");
    let names = property(node, "reset-names");
    let words = prop
        .as_ref()
        .map_or(0, |prop| prop.length() / size_of::<u32>());
    let mut index = 0;
    let mut position = 0;
    core::iter::from_fn(move || {
        let prop = prop.as_ref()?;
        if index >= words {
            return None;
        }
        let controller = prop.u32(index).ok()?;
        let cells = property(&node_by_phandle(tree, controller)?, "#reset-cells")
            .and_then(|cells| cells.u32(0).ok())?;
        let id = match cells {
            0 => 0,
            1 => prop.u32(index + 1).ok()?,
            _ => return None,
        };
        index += 1 + cells as usize;
        let name = names
            .as_ref()
            .and_then(|names| names.iter_str().nth(position).ok().flatten())
            .unwrap_or("");
        position += 1;
        Some(Reset {
            controller,
            id,
            name,
        })
    })
}
