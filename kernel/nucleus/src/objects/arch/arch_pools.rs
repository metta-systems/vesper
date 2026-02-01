use crate::objects::{ArchObjects, ObjectPool};

// ═══════════════════════════════════════════════════════════════════
// ARCHITECTURE-SPECIFIC OBJECT POOLS
// ═══════════════════════════════════════════════════════════════════

/// Pools for architecture-specific objects
pub struct ArchPools<A: ArchObjects> {
    // pub frames: ObjectPool<A::Frame>,
    // pub page_tables: ObjectPool<A::PageTable>,
    // pub vspaces: ObjectPool<A::VSpace>,
    // pub asid_pools: ObjectPool<A::ASIDPool>,
    // pub asids: ObjectPool<A::ASID>,
    _marker: core::marker::PhantomData<A>, // FIXME temp
}

impl<A: ArchObjects> ArchPools<A> {
    /// Create pools backed by untyped memory
    ///
    /// # Safety
    /// Memory regions must be valid and non-overlapping
    pub unsafe fn new(// frame_mem: (*mut u8, usize),
        // pt_mem: (*mut u8, usize),
        // vspace_mem: (*mut u8, usize),
        // asid_pool_mem: (*mut u8, usize),
        // asid_mem: (*mut u8, usize),
    ) -> Self {
        Self {
            // frames: unsafe { ObjectPool::new(frame_mem.0, frame_mem.1) },
            // page_tables: unsafe { ObjectPool::new(pt_mem.0, pt_mem.1) },
            // vspaces: unsafe { ObjectPool::new(vspace_mem.0, vspace_mem.1) },
            // asid_pools: unsafe { ObjectPool::new(asid_pool_mem.0, asid_pool_mem.1) },
            // asids: unsafe { ObjectPool::new(asid_mem.0, asid_mem.1) },
            _marker: core::marker::PhantomData,
        }
    }
}
