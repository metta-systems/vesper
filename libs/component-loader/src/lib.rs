#![no_std]

//! Boot-time component provisioning, shared by Kickstart and the e2e test
//! kernels.
//!
//! The boot Thread builds translation contexts and loads components through
//! ordinary capability invocations on its own table (`Untyped.Retype`,
//! `PageTable.Map`, `Frame.Map`, `CopyDerive`, `Move`); the direct boot-table
//! address is used only for bootstrap-origin grants and read-only lookups.
//!
//! - [`keys`]: the boot table's layout and key/slot composition;
//! - [`retained`]: the retained init image and stack, granted as Frames;
//! - [`paging`]: page-table walking and retained-image geometry;
//! - [`builder`]: page-table chains, Frames, the retained image mapped into
//!   several roots, high execution stacks;
//! - [`component`]: a complete component `AddressSpace` with its own table,
//!   root and ASID;
//! - [`loader`]: loading bundled EL0 components and their guarded stacks.

pub mod builder;
pub mod component;
pub mod keys;
pub mod loader;
pub mod paging;
pub mod retained;
