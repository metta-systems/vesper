//! CPUs and how to start them: the `/cpus` node (`DTSpec` v0.3 §3.7, §3.8) and the PSCI
//! node.
//!
//! - Every direct child of `/cpus` with `device_type = "cpu"` is a CPU, in device tree
//!   order; that order is the logical CPU index. Other children (`cpu-map`, cache nodes)
//!   are not CPUs. A CPU with `status = "disabled"` is still listed: it is quiescent and
//!   can be started through its `enable-method` (§3.8.1).
//! - A CPU's `reg` holds its `MPIDR_EL1` affinity: one cell for Aff2..Aff0 (bits 23:0), or
//!   two cells with Aff3 in the first, which read as one number land in the register's own
//!   layout (Aff3 in bits 39:32).
//! - `enable-method` names the start method: `spin-table` with a 64-bit `cpu-release-addr`,
//!   or `psci`, which uses the PSCI node. The Raspberry Pi 3 and 4 trees also give
//!   `/cpus` itself a 32-bit `brcm,bcm2836-smp` method; only the per-CPU value is read.

use crate::{DevTreeIndex, Node, PropReader, find_compatible, property, reg};

/// `enable-method` of a CPU released by writing its entry to `cpu-release-addr`.
pub const SPIN_TABLE: &str = "spin-table";
/// `enable-method` of a CPU started by PSCI `CPU_ON`.
pub const PSCI: &str = "psci";

/// `compatible` strings of the PSCI node, newest first. 0.2 and later use the standard
/// function IDs; 0.1 (`arm,psci`) names its own in properties.
pub const PSCI_COMPATIBLES: &[&str] = &["arm,psci-1.0", "arm,psci-0.2", "arm,psci"];
/// The PSCI 0.1 `compatible`, whose function IDs the node lists.
const PSCI_0_1: &str = "arm,psci";

/// PSCI `CPU_ON`, SMC64 calling convention: the standard function ID from PSCI 0.2 on.
pub const PSCI_CPU_ON_64: u32 = 0xC400_0003;

/// One CPU of the `/cpus` node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cpu<'dt> {
    /// Its position among the CPUs of `/cpus`: the logical CPU index.
    pub index: usize,
    /// Its `reg`: the `MPIDR_EL1` affinity bits, in the register's layout.
    pub affinity: u64,
    /// Its `enable-method` (the first string), if it has one.
    pub enable_method: Option<&'dt str>,
    /// Its `cpu-release-addr`, if it has one: the word a spin table polls.
    pub release_address: Option<u64>,
}

/// How PSCI calls reach the firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsciConduit {
    /// `SMC`, to EL3 firmware.
    Smc,
    /// `HVC`, to a hypervisor.
    Hvc,
}

/// The PSCI node: how to call the firmware and which function starts a CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Psci<'s> {
    /// The `compatible` string matched.
    pub compatible: &'s str,
    pub conduit: PsciConduit,
    /// The `CPU_ON` function ID: standard from 0.2 on, the node's `cpu_on` for 0.1.
    pub cpu_on: u32,
}

/// The `/cpus` node.
fn cpus_node<'a, 'i: 'a, 'dt: 'i>(tree: &'a DevTreeIndex<'i, 'dt>) -> Option<Node<'a, 'i, 'dt>> {
    tree.root()
        .children()
        .find(|child| child.name() == Ok("cpus"))
}

/// Whether `node` is a CPU node.
fn is_cpu(node: &Node<'_, '_, '_>) -> bool {
    property(node, "device_type").is_some_and(|prop| prop.str() == Ok("cpu"))
}

/// The CPU described by `node`, which is the `index`th CPU of `/cpus`; `None` when it has
/// no `reg`.
fn cpu<'dt>(node: &Node<'_, '_, 'dt>, index: usize) -> Option<Cpu<'dt>> {
    let affinity = reg(node).next()?.start;
    let enable_method = property(node, "enable-method").and_then(|prop| prop.str().ok());
    let release_address = property(node, "cpu-release-addr").and_then(|prop| {
        let high = u64::from(prop.u32(0).ok()?);
        let low = u64::from(prop.u32(1).ok()?);
        Some(high << 32 | low)
    });
    Some(Cpu {
        index,
        affinity,
        enable_method,
        release_address,
    })
}

/// The CPUs of `/cpus`, in device tree order (their logical indices). A CPU node without a
/// `reg` is skipped and takes no index.
pub fn cpus<'a, 'i: 'a, 'dt: 'i>(
    tree: &'a DevTreeIndex<'i, 'dt>,
) -> impl Iterator<Item = Cpu<'dt>> + 'a {
    cpus_node(tree)
        .into_iter()
        .flat_map(|cpus| cpus.children())
        .filter(is_cpu)
        .filter_map(|node| cpu(&node, 0))
        .enumerate()
        .map(|(index, found)| Cpu { index, ..found })
}

/// The PSCI node, if the tree has one with a known `method`.
pub fn psci(tree: &DevTreeIndex<'_, '_>) -> Option<Psci<'static>> {
    let (node, compatible) = find_compatible(tree, PSCI_COMPATIBLES)?;
    let conduit = match property(&node, "method")?.str().ok()? {
        "smc" => PsciConduit::Smc,
        "hvc" => PsciConduit::Hvc,
        _ => return None,
    };
    let cpu_on = if compatible == PSCI_0_1 {
        property(&node, "cpu_on")?.u32(0).ok()?
    } else {
        PSCI_CPU_ON_64
    };
    Some(Psci {
        compatible,
        conduit,
        cpu_on,
    })
}

/// The `enable-method` every one of `cpus` names; `None` when there are no CPUs, one has
/// none, or two disagree.
pub fn common_enable_method<'dt>(mut cpus: impl Iterator<Item = Cpu<'dt>>) -> Option<&'dt str> {
    let first = cpus.next()?.enable_method?;
    cpus.all(|other| other.enable_method == Some(first))
        .then_some(first)
}
