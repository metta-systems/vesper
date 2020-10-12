/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

#[cfg(target_arch = "aarch64")]
#[macro_use]
pub mod aarch64;
#[cfg(target_arch = "aarch64")]
pub use self::aarch64::*;
