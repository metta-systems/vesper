use crate::objects::{ArchObjects, ObjectPool};

// ═══════════════════════════════════════════════════════════════════
// ARCHITECTURE-SPECIFIC OBJECT POOLS
// ═══════════════════════════════════════════════════════════════════

/// Pools for architecture-specific objects.
///
/// Pool backing is explicitly carved and charged (boot Untyped or Retype), never
/// a silent second source of kernel memory. Page tables (the 4 KiB tables and
/// their kernel metadata records) and `AddressSpace`s are carved from Untyped
/// memory and named by their carved identity; they take no pool slot.
pub struct ArchPools<A: ArchObjects> {
    /// ASID pools: boot-provided capability-protected namespace resources
    /// (selected 2026-09-15); not Retype-creatable, so the only backing today
    /// is the boot pool carved at bootstrap.
    pub asid_pools: ObjectPool<A::ASIDPool>,
    // Pools for ASIDControl/I/O remain deferred with their kinds: no
    // creatable arch kind other than Frame and PageTable is allowlisted, so
    // no backing is carved for them yet.
    pub _marker: core::marker::PhantomData<A>,
}

impl<A: ArchObjects> ArchPools<A> {
    /// Create the arch pools with explicitly carved backings.
    ///
    /// # Safety
    /// `asid_pools` must be backed by memory exclusively owned by this pool:
    /// carved from an Untyped's committed range and never freed.
    pub unsafe fn new(asid_pools: ObjectPool<A::ASIDPool>) -> Self {
        Self {
            asid_pools,
            _marker: core::marker::PhantomData,
        }
    }
}
