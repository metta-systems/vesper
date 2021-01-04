/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

use snafu::Snafu;

// The source of all available memory, device or general.
// Boot code reserves kernel memory and initial mapping allocations (4 pages probably - on rpi3? should be platform-dependent).
// The rest is converted to untypeds with appropriate kind and given away to start thread.

// Untyped.retype() derives cap to a typed cap (derivation tree must be maintained)

pub(crate) struct Untyped {}

impl super::NucleusObject for Untyped {
    fn size_bits() -> usize {
        unimplemented!()
    }

    fn invoke() {
        unimplemented!()
    }
}

#[derive(Debug, Snafu)]
enum RetypeError {
    Whatever,
}

impl Untyped {
    // Uses T::SIZE_BITS to properly size the resulting object
    // in some cases size_bits must be passed as argument though...
    // @todo return an array of caps?
    fn retype<T: NucleusObject>(
        target_cap: CapNodeRootedPath,
        target_cap_offset: usize,
        num_objects: usize,
    ) -> Result<CapSlice, RetypeError> {
        Err(RetypeError::Whatever)
    }
}

enum MemoryKind {
    General,
    Device,
}

// with GATs
// trait Retyped { type Result = CapTable::<T> .. }
