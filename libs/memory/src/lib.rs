/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Translation table management for the Vesper nanokernel.
//!
//! This library provides arch-independent abstractions for MMU translation
//! tables. Each table level is a first-class object, matching the kernel's
//! capability-based syscall API where GlobalDirectory, PageDirectory, Frame
//! etc. are separate capabilities.
//!
//! All table memory is externally provided — this library never allocates.

#![no_std]
#![allow(internal_features)]
#![feature(core_intrinsics)] // internal feature
#![feature(format_args_nl)]

pub mod arch_trait;
pub mod error;
pub mod table;
pub mod walk;

mod arch;

// Re-export core types at crate root.
pub use {
    arch_trait::{EntryKind, LevelCapabilities, TranslationArch},
    error::TableError,
    table::{Table, TableRef},
    walk::{TranslationResult, translate, translate_hashed},
};

// Re-export arch implementations.
#[cfg(target_arch = "aarch64")]
pub use arch::aarch64::{Aarch64_4K, features, mmu};

pub use arch::{powerpc::PowerPC_970, riscv64::RiscV_Sv48, x86_64::X86_64_4K};
