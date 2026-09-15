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
    // Pools for VSpace/ASIDPool/ASID remain deferred with their kinds: no
    // creatable arch kind other than Frame and PageTable is allowlisted, so
    // no backing is carved for them yet.
    pub _marker: core::marker::PhantomData<A>,
}

impl<A: ArchObjects> ArchPools<A> {
    /// Create the arch pools with an explicitly carved page-table pool.
    ///
    /// # Safety
    /// `page_tables` must be backed by memory exclusively owned by this pool:
    /// carved from an Untyped's committed range and never freed.
    pub unsafe fn new(page_tables: ObjectPool<A::PageTable>) -> Self {
        Self {
            page_tables,
            _marker: core::marker::PhantomData,
        }
    }
}
