use crate::objects::{ArchObjects, ObjectPool};

// ═══════════════════════════════════════════════════════════════════
// ARCHITECTURE-SPECIFIC OBJECT POOLS
// ═══════════════════════════════════════════════════════════════════

/// Pools for architecture-specific objects.
///
/// Pool backing is explicitly carved and charged (boot Untyped or Retype), never
/// a silent second source of kernel memory. The page-table pool holds only
/// kernel metadata (carve address, installation record); the 4 KiB tables
/// themselves are Retype carves charged to the invoking Untyped.
pub struct ArchPools<A: ArchObjects> {
    pub page_tables: ObjectPool<A::PageTable>,
    /// Address spaces: the protection/mapping-context objects (the renamed
    /// `VSpace` kind, split from the former `Domain` 2026-09-21). Boot-carved;
    /// not Retype-creatable yet.
    pub address_spaces: ObjectPool<A::AddressSpace>,
    /// ASID pools: boot-provided capability-protected namespace resources
    /// (selected 2026-09-15); not Retype-creatable, so the only backing today
    /// is the boot pool carved at bootstrap.
    pub asid_pools: ObjectPool<A::ASIDPool>,
    // Pools for ASIDControl/I/O/IRQ remain deferred with their kinds: no
    // creatable arch kind other than Frame and PageTable is allowlisted, so
    // no backing is carved for them yet.
    pub _marker: core::marker::PhantomData<A>,
}

impl<A: ArchObjects> ArchPools<A> {
    /// Create the arch pools with explicitly carved backings.
    ///
    /// # Safety
    /// `page_tables`, `address_spaces`, and `asid_pools` must be backed by
    /// memory exclusively owned by this pool: carved from an Untyped's
    /// committed range and never freed.
    pub unsafe fn new(
        page_tables: ObjectPool<A::PageTable>,
        address_spaces: ObjectPool<A::AddressSpace>,
        asid_pools: ObjectPool<A::ASIDPool>,
    ) -> Self {
        Self {
            page_tables,
            address_spaces,
            asid_pools,
            _marker: core::marker::PhantomData,
        }
    }
}
