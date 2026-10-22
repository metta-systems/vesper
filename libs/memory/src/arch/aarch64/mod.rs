/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Memory management functions for aarch64.

mod common;
mod granule_16k;
mod granule_4k;
mod granule_64k;

pub mod features;
pub mod mmu;

pub use {granule_4k::Aarch64_4K, granule_16k::Aarch64_16K, granule_64k::Aarch64_64K};
