#![no_std]
#![no_main]
#![allow(stable_features)]
#![allow(incomplete_features)]
#![allow(internal_features)]

#[cfg(not(target_arch = "aarch64"))]
use architecture_not_supported_sorry;

/// Architecture-specific code.
#[macro_use]
pub mod arch;
pub mod memory;
