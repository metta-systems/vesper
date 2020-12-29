/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

use {
    super::{CapError, Capability, PhysAddr, TryFrom},
    crate::capdef,
    paste::paste,
    register::{register_bitfields, LocalRegisterCopy},
};

//=====================
// Cap definition
//=====================

register_bitfields! {
    u128,
    // The combination of freeIndex and blockSize must match up with the
    // definitions of MIN_SIZE_BITS and MAX_SIZE_BITS
    // -- https://github.com/seL4/seL4/blob/master/include/object/structures_32.bf#L18
    //
    // /* It is assumed that every untyped is within seL4_MinUntypedBits and seL4_MaxUntypedBits
    //  * (inclusive). This means that every untyped stored as seL4_MinUntypedBits
    //  * subtracted from its size before it is stored in capBlockSize, and
    //  * capFreeIndex counts in chunks of size 2^seL4_MinUntypedBits. The seL4_MaxUntypedBits
    //  * is the minimal untyped that can be stored when considering both how
    //  * many bits of capBlockSize there are, and the largest offset that can
    //  * be stored in capFreeIndex */
    // +#define MAX_FREE_INDEX(sizeBits) (BIT( (sizeBits) - seL4_MinUntypedBits ))
    // +#define FREE_INDEX_TO_OFFSET(freeIndex) ((freeIndex)<<seL4_MinUntypedBits)
    // #define GET_FREE_REF(base,freeIndex) ((word_t)(((word_t)(base)) + FREE_INDEX_TO_OFFSET(freeIndex)))
    // #define GET_FREE_INDEX(base,free) (((word_t)(free) - (word_t)(base))>>seL4_MinUntypedBits)
    // #define GET_OFFSET_FREE_PTR(base, offset) ((void *)(((word_t)(base)) + (offset)))
    // +#define OFFSET_TO_FREE_INDEX(offset) ((offset)>>seL4_MinUntypedBits)
    //
    // exception_t decodeUntypedInvocation(word_t invLabel, word_t length,
    //                                     cte_t *slot, cap_t cap,
    //                                     extra_caps_t excaps, bool_t call,
    //                                     word_t *buffer);
    // exception_t invokeUntyped_Retype(cte_t *srcSlot, bool_t reset,
    //                                  void *retypeBase, object_t newType,
    //                                  word_t userSize, slot_range_t destSlots,
    //                                  bool_t deviceMemory);
    // // -- https://github.com/seL4/seL4/blob/master/src/object/untyped.c#L276
    // -- https://github.com/seL4/seL4/blob/master/include/object/untyped.h
    //
    // /* Untyped size limits */
    // #define seL4_MinUntypedBits 4
    // #define seL4_MaxUntypedBits 47
    // -- https://github.com/seL4/seL4/blob/master/libsel4/sel4_arch_include/aarch64/sel4/sel4_arch/constants.h#L234
    //
    // /*
    //  * Determine where in the Untyped region we should start allocating new
    //  * objects.
    //  *
    //  * If we have no children, we can start allocating from the beginning of
    //  * our untyped, regardless of what the "free" value in the cap states.
    //  * (This may happen if all of the objects beneath us got deleted).
    //  *
    //  * If we have children, we just keep allocating from the "free" value
    //  * recorded in the cap.
    //  */
    // -- https://github.com/seL4/seL4/blob/master/src/object/untyped.c#L175
    // /*
    //  * Determine the maximum number of objects we can create, and return an
    //  * error if we don't have enough space.
    //  *
    //  * We don't need to worry about alignment in this case, because if anything
    //  * fits, it will also fit aligned up (by packing it on the right hand side
    //  * of the untyped).
    //  */
    // -- https://github.com/seL4/seL4/blob/master/src/object/untyped.c#L196

    UntypedCap [
        Type OFFSET(0) NUMBITS(6) [
            value = 2
        ],
        /// Index of the first unoccupied byte within this Untyped.
        /// This index is limited between MIN_UNTYPED_BITS and max bits number in BlockSizePower.
        /// To occupy less bits, the free index is shifted right by MIN_UNTYPED_BITS.
        ///
        /// Free index is used only if this untyped has children, which may be occupying only
        /// part of its space.
        /// This means an Untyped can be retyped multiple times as long as there is
        /// free space left in it.
        FreeIndexShifted OFFSET(0) NUMBITS(48) [],
        /// Device mapped untypeds cannot be touched by the kernel.
        IsDevice OFFSET(57) NUMBITS(1) [],
        /// Untyped is 2**BlockSizePower bytes in size
        BlockSizePower OFFSET(58) NUMBITS(6) [],
        /// Physical address of untyped.
        Ptr OFFSET(80) NUMBITS(48) [],
    ]
}

capdef! { Untyped }

//=====================
// Cap implementation
//=====================

// @todo retyping a device capability requires specifying memory base exactly, can't just pick next frame?

/// Capability to a block of untyped memory.
/// Can be retyped into more usable types.
impl UntypedCapability {
    const MIN_BITS: usize = 4;
    const MAX_BITS: usize = 47;

    /// This untyped belongs to device memory (will not be zeroed on allocation).
    pub fn is_device(&self) -> bool {
        self.0.read(UntypedCap::IsDevice) == 1
    }

    /// Return untyped block size in bytes.
    pub fn block_size(&self) -> usize {
        1 << self.0.read(UntypedCap::BlockSizePower)
    }
    // FreeIndex OFFSET(0) NUMBITS(48) [],
    /// Return free area offset in this block in bytes.
    pub fn free_area_offset(&self) -> usize {
        use core::convert::TryInto;
        Self::free_index_to_offset(
            self.0
                .read(UntypedCap::FreeIndexShifted)
                .try_into()
                .unwrap(),
        )
    }

    /// Return start address of this untyped block.
    pub fn base(&self) -> PhysAddr {
        (self.0.read(UntypedCap::Ptr) as u64).into() // @todo implement TryFrom<u128> for PhysAddr
    }

    // #define MAX_FREE_INDEX(sizeBits) (BIT( (sizeBits) - seL4_MinUntypedBits ))
    /// Calculate maximum free index value based on allowed size bits.
    pub fn max_free_index_from_bits(size_bits: usize) -> usize {
        assert!(size_bits >= Self::MIN_BITS);
        assert!(size_bits <= Self::MAX_BITS);
        1 << (size_bits - Self::MIN_BITS)
    }

    // #define FREE_INDEX_TO_OFFSET(freeIndex) ((freeIndex)<<seL4_MinUntypedBits)
    /// Convert free index to byte offset.
    fn free_index_to_offset(index: usize) -> usize {
        index << Self::MIN_BITS
    }

    // #define OFFSET_TO_FREE_INDEX(offset) ((offset)>>seL4_MinUntypedBits)
    /// Convert byte offset to free index.
    /// @todo Check proper offset alignment!
    fn offset_to_free_index(offset: usize) -> usize {
        offset >> Self::MIN_BITS
    }
}
