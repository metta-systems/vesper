//! An indexed device tree and the boot-time views of it.

use {
    crate::{
        DevTree, DevTreeError, DevTreeIndex, Node, Prop, PropReader, Region, is_enabled, phandle,
        property, regions,
    },
    core::alloc::Layout,
};

/// An indexed flattened device tree.
///
/// The index lives in a caller-provided buffer of [`DeviceTree::index_layout`]
/// (at boot, a bump allocation), so the tree can be parsed before any heap
/// exists.
pub struct DeviceTree<'i, 'dt> {
    index: DevTreeIndex<'i, 'dt>,
}

/// A device: one translated `reg` entry of a node that is not memory.
#[derive(Clone)]
pub struct Device<'a, 'i, 'dt> {
    pub node: Node<'a, 'i, 'dt>,
    /// Node name without the unit address
    pub name: &'dt str,
    /// The most specific `compatible` string, or empty
    pub compatible: &'dt str,
    pub phandle: Option<u32>,
    /// Whether the node's `status` is okay
    pub enabled: bool,
    /// The entry, as a CPU physical address range
    pub region: Region,
}

/// Whether `node` describes memory (`device_type = "memory"`).
fn is_memory(node: &Node<'_, '_, '_>) -> bool {
    property(node, "device_type").is_some_and(|prop| prop.str() == Ok("memory"))
}

/// Whether node `name` matches path component `component`: exactly, or by
/// its name without the unit address when `component` has none.
fn matches_component(name: &str, component: &str) -> bool {
    name == component
        || (!component.contains('@')
            && name
                .split_once('@')
                .is_some_and(|(base, _)| base == component))
}

impl<'i, 'dt> DeviceTree<'i, 'dt> {
    /// The blob at `pointer`, after checking its header.
    ///
    /// # Safety
    ///
    /// `pointer` must be 32-bit aligned and point at a flattened device tree
    /// that stays valid and unmodified for `'dt`.
    ///
    /// # Errors
    ///
    /// If the header is not a valid device tree header.
    pub unsafe fn blob_from_raw(pointer: *const u8) -> Result<DevTree<'dt>, DevTreeError> {
        // SAFETY: the caller's contract.
        unsafe { DevTree::from_raw_pointer(pointer) }
    }

    /// The size and alignment of the index buffer for `blob`.
    ///
    /// # Errors
    ///
    /// If the blob's structure cannot be walked.
    pub fn index_layout(blob: &DevTree<'dt>) -> Result<Layout, DevTreeError> {
        DevTreeIndex::get_layout(blob)
    }

    /// Index `blob` into `buffer` (at least [`Self::index_layout`]).
    ///
    /// # Errors
    ///
    /// If the buffer is too small or the blob is malformed.
    pub fn new(blob: DevTree<'dt>, buffer: &'i mut [u8]) -> Result<Self, DevTreeError> {
        Ok(Self {
            index: DevTreeIndex::new(blob, buffer)?,
        })
    }

    /// The underlying index, for the node-level helpers of this crate.
    pub fn index(&self) -> &DevTreeIndex<'i, 'dt> {
        &self.index
    }

    /// The blob's size in bytes.
    pub fn total_size(&self) -> usize {
        self.index.fdt().totalsize()
    }

    /// The node at `path` (`/`-separated from the root; a component may omit
    /// the unit address).
    pub fn node(&self, path: &str) -> Option<Node<'_, 'i, 'dt>> {
        path.split('/')
            .filter(|component| !component.is_empty())
            .try_fold(self.index.root(), |node, component| {
                node.children().find(|child| {
                    child
                        .name()
                        .is_ok_and(|name| matches_component(name, component))
                })
            })
    }

    /// The property at `path`: a node path followed by the property name.
    pub fn property(&self, path: &str) -> Option<Prop<'_, 'i, 'dt>> {
        let (node, name) = path.rsplit_once('/')?;
        property(&self.node(node)?, name)
    }

    /// The board's `model` string.
    pub fn model(&self) -> Option<&'dt str> {
        property(&self.index.root(), "model").and_then(|prop| prop.str().ok())
    }

    /// Usable RAM: the non-empty `reg` entries of every memory node.
    pub fn memory(&self) -> impl Iterator<Item = Region> + '_ {
        self.index
            .nodes()
            .filter(is_memory)
            .flat_map(|node| regions(&node))
            .filter(|region| region.size != 0)
    }

    /// Memory the boot firmware reserved (`/memreserve/` entries).
    pub fn reserved_memory(&self) -> impl Iterator<Item = Region> + '_ {
        self.index.fdt().reserved_entries().map(|entry| Region {
            start: entry.address.into(),
            size: entry.size.into(),
        })
    }

    /// Every translated `reg` entry of every node that is not memory.
    pub fn devices(&self) -> impl Iterator<Item = Device<'_, 'i, 'dt>> {
        self.index
            .nodes()
            .filter(|node| !is_memory(node))
            .flat_map(|node| {
                let name = node.name().unwrap_or("");
                let name = name.split_once('@').map_or(name, |(base, _)| base);
                let compatible = property(&node, "compatible")
                    .and_then(|prop| prop.str().ok())
                    .unwrap_or("");
                let phandle = phandle(&node);
                let enabled = is_enabled(&node);
                let owner = node.clone();
                regions(&node).map(move |region| Device {
                    node: owner.clone(),
                    name,
                    compatible,
                    phandle,
                    enabled,
                    region,
                })
            })
    }
}
