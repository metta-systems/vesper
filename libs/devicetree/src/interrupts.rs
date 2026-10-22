//! Interrupt topology (`DTSpec` v0.3 §2.4): the CPU's generic timer, the
//! root interrupt controller it is wired to, and each node's interrupts
//! resolved to the controller they are routed to and their specifier cells.
//!
//! A node lists its interrupts either in `interrupts`, every entry relative
//! to its (inherited) `interrupt-parent`, or in `interrupts-extended`, where
//! each entry names its controller by phandle first. Either way an entry is
//! as many cells wide as its controller's `#interrupt-cells`.

use {
    crate::{
        DevTreeIndex, Node, PropReader, find_compatible, interrupt_cells, interrupt_parent,
        node_by_phandle, phandle, property,
    },
    core::iter,
};

/// `compatible` strings of the ARM generic timer node.
pub const GENERIC_TIMERS: &[&str] = &["arm,armv8-timer", "arm,armv7-timer"];

/// The generic timer's interrupts, by their position in its `interrupts`
/// (the `arm,armv8-timer` binding order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum TimerInterrupt {
    /// Secure EL1 physical timer
    SecurePhysical = 0,
    /// Non-secure EL1 physical timer
    NonSecurePhysical = 1,
    /// Virtual timer
    Virtual = 2,
    /// EL2 (hypervisor) physical timer
    Hypervisor = 3,
}

/// Most cells of one interrupt specifier this crate decodes.
pub const MAX_INTERRUPT_CELLS: usize = 4;

/// One interrupt of a node: the controller it is routed to and its
/// specifier, relative to that controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interrupt {
    /// The controller's phandle
    pub controller: u32,
    /// Number of valid entries in `cells`: the controller's `#interrupt-cells`
    pub cell_count: u32,
    pub cells: [u32; MAX_INTERRUPT_CELLS],
}

impl Interrupt {
    /// The specifier's cells.
    pub fn cells(&self) -> &[u32] {
        &self.cells[..(self.cell_count as usize).min(MAX_INTERRUPT_CELLS)]
    }
}

/// The ARM generic timer node.
pub fn generic_timer<'a, 'i: 'a, 'dt: 'i>(
    tree: &'a DevTreeIndex<'i, 'dt>,
) -> Option<Node<'a, 'i, 'dt>> {
    find_compatible(tree, GENERIC_TIMERS).map(|(timer, _)| timer)
}

/// The root interrupt controller: the one the generic timer is wired to.
pub fn root_interrupt_controller<'a, 'i: 'a, 'dt: 'i>(
    tree: &'a DevTreeIndex<'i, 'dt>,
) -> Option<Node<'a, 'i, 'dt>> {
    interrupt_parent(tree, &generic_timer(tree)?)
}

/// The generic timer's interrupt `which`.
pub fn timer_interrupt(tree: &DevTreeIndex<'_, '_>, which: TimerInterrupt) -> Option<Interrupt> {
    interrupt(tree, &generic_timer(tree)?, which as usize)
}

/// The specifier width of the controller with `phandle`, if it fits.
fn controller_cells(tree: &DevTreeIndex<'_, '_>, controller: u32) -> Option<usize> {
    let cells = interrupt_cells(&node_by_phandle(tree, controller)?)? as usize;
    (cells <= MAX_INTERRUPT_CELLS).then_some(cells)
}

/// `count` cells of `prop` starting at cell `first`.
fn read_cells<'a, 'i: 'a, 'dt: 'i>(
    prop: &crate::Prop<'a, 'i, 'dt>,
    first: usize,
    count: usize,
) -> Option<[u32; MAX_INTERRUPT_CELLS]> {
    let mut cells = [0; MAX_INTERRUPT_CELLS];
    for (position, cell) in cells.iter_mut().take(count).enumerate() {
        *cell = prop.u32(first + position).ok()?;
    }
    Some(cells)
}

/// `node`'s interrupt at position `index`, from `interrupts-extended` if it
/// has one, otherwise from `interrupts` relative to its `interrupt-parent`.
pub fn interrupt(
    tree: &DevTreeIndex<'_, '_>,
    node: &Node<'_, '_, '_>,
    index: usize,
) -> Option<Interrupt> {
    if let Some(prop) = property(node, "interrupts-extended") {
        // Entries are `<phandle cells...>`, each as wide as its controller's
        // `#interrupt-cells`: walk them in order.
        let mut position = 0;
        for current in 0..=index {
            let controller = prop.u32(position).ok()?;
            let count = controller_cells(tree, controller)?;
            if current == index {
                return Some(Interrupt {
                    controller,
                    cell_count: u32::try_from(count).ok()?,
                    cells: read_cells(&prop, position + 1, count)?,
                });
            }
            position += 1 + count;
        }
        return None;
    }
    let prop = property(node, "interrupts")?;
    let controller = phandle(&interrupt_parent(tree, node)?)?;
    let count = controller_cells(tree, controller)?;
    Some(Interrupt {
        controller,
        cell_count: u32::try_from(count).ok()?,
        cells: read_cells(&prop, index * count, count)?,
    })
}

/// Every interrupt of `node`, in order.
pub fn interrupts<'t>(
    tree: &'t DevTreeIndex<'_, '_>,
    node: &'t Node<'_, '_, '_>,
) -> impl Iterator<Item = Interrupt> + 't {
    (0..).map_while(move |index| interrupt(tree, node, index))
}

/// Interrupt-controller nodes in `tree`, the root first, then the others
/// for which `drives` holds, in device tree order: the order
/// `IRQControl.Issue` numbers a component's controllers by.
pub fn interrupt_controllers<'a, 'i: 'a, 'dt: 'i>(
    tree: &'a DevTreeIndex<'i, 'dt>,
    mut drives: impl FnMut(&Node<'a, 'i, 'dt>) -> bool,
) -> impl Iterator<Item = Node<'a, 'i, 'dt>> {
    let root = root_interrupt_controller(tree);
    let root_phandle = root.as_ref().and_then(phandle);
    let cascaded = tree.nodes().filter(move |node| {
        crate::is_interrupt_controller(node)
            && crate::is_enabled(node)
            && (root_phandle.is_none() || phandle(node) != root_phandle)
            && drives(node)
    });
    iter::once(root).flatten().chain(cascaded)
}
