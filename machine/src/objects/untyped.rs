/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

pub(crate) struct Untyped {}

impl super::KernelObject for Untyped {
    fn size_bits() -> usize {
        unimplemented!()
    }

    fn invoke() {
        unimplemented!()
    }
}

impl Untyped {
    fn retype() {}
}

enum MemoryKind {
    General,
    Device,
}

// The source of all available memory, device or general.
// Boot code reserves kernel memory and initial mapping allocations (4 pages probably - on rpi3? should be platform-dependent).
// The rest is converted to untypeds with appropriate kind and given away to start thread.

// Untyped.retype() derives cap to a typed cap (derivation tree must be maintained)

trait Untyped {
    // Uses T::SIZE_BITS to properly size the resulting object
    // in some cases size_bits must be passed as argument though...
    fn retype<T: NucleusObject>(
        target_cap: CapNodeRootedPath,
        target_cap_offset: usize,
        num_objects: usize,
    ) -> Result<CapSlice>; // @todo return an array of caps?
}

// with GATs
// trait Retyped { type Result = CapTable::<T> .. }
