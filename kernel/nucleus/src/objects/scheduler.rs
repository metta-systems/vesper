//! `Scheduler` kernel object: authority to make Threads run.
//!
//! Kickstart carves the one root Scheduler from the boot Untyped and installs
//! it at `KeySlot::ROOT_SCHEDULER`. Only the root configures and starts
//! Threads created by `Untyped.Retype` (Composite's root-only thread creation,
//! expressed as activation). Scheduler derivation, donation and the shared
//! scheduling records are not implemented yet; `Untyped.Retype` cannot create
//! a Scheduler.

use {
    crate::objects::{NucleusObject, carved::CarvedObject},
    libobject::ObjectType,
};

/// A scheduler in the scheduler tree.
#[derive(Clone, Copy, Debug)]
pub struct Scheduler {
    root: bool,
}

impl Scheduler {
    /// The root scheduler; only boot code creates it.
    pub const fn root() -> Self {
        Self { root: true }
    }

    /// Whether this is the root scheduler.
    pub const fn is_root(&self) -> bool {
        self.root
    }
}

impl NucleusObject for Scheduler {
    const TYPE: ObjectType = ObjectType::SCHEDULER;
}

impl CarvedObject for Scheduler {}
