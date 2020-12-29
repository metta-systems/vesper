/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Syscall ABI for calling kernel functions.
//!
//! Principally, there are two syscalls - one does not use capabilities, `Yield` and one is performing
//! a capability invocation, `InvokeCapability`. However internally the invocation is dispatched to
//! multiple available kernel functions, specific to each capability.

/// Parse syscall and invoke API functions.
///
/// Implements C ABI to easily parse passed in parameters.
/// @todo Move this to aarch64-specific part.
#[no_mangle]
extern "C" pub(crate) syscall_entry() {}
