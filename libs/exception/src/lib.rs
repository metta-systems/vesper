// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2020-2022 Andre Richter <andre.o.richter@gmail.com>

//! Synchronous and asynchronous exception handling.

#![no_std]
#![no_main]
#![feature(format_args_nl)]
#![feature(stmt_expr_attributes)]

pub mod arch;
pub mod asynchronous;

#[cfg(target_arch = "aarch64")]
use crate::arch::aarch64 as arch_exception;

//--------------------------------------------------------------------------------------------------
// Architectural Public Reexports
//--------------------------------------------------------------------------------------------------
pub use arch_exception::{current_privilege_level, handling_init};

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Kernel privilege levels.
#[allow(missing_docs)]
#[derive(Eq, PartialEq)]
pub enum PrivilegeLevel {
    User,
    Kernel,
    Hypervisor,
    Unknown,
}
