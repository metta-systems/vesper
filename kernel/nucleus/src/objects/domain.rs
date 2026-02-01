use {
    crate::objects::{KeyTable, NucleusObject},
    core::{ptr::NonNull, sync::atomic::Ordering},
    libmemory::{phys_addr::PhysAddr, virt_addr::VirtAddr},
    libobject::{
        ObjectType,
        domain::{DcbPage, DomainControlBlock, DomainId, DomainState},
    },
};

// ====================
// == Nucleus object ==
// ====================

/// This is a nucleus-visible half of domain structure.
/// The DomainControlBlock is user-visible and is defined in libobject.
pub struct Domain {
    // ═══════════════════════════════════════════════════════════
    // PRIVATE SECTION (kernel only, NOT mapped to userspace)
    // ═══════════════════════════════════════════════════════════
    //
    // This would be in a separate structure or after a page boundary
    // - Saved register context
    // - Capability space (keytable)
    // - Kernel stack pointer
    // - Etc.
    pub keytable: KeyTable,
}

// Verify size for cache alignment
// TODO const _: () = assert!(core::mem::size_of::<Domain>() == 4096);

impl NucleusObject for Domain {
    const TYPE: ObjectType = ObjectType::DOMAIN;
}

impl Domain {
    // Initialize new domain's cspace
    // fn init_cspace(&mut self) {
    //     // Slot 0: capability to this captbl itself
    //     self.cspace[CAPTBL_SELF] = Cap::new(ObjectType::KeyTable, self.cspace_id);
    //     // Now domain can manipulate its own caps
    // }
}

// ## Memory Ordering Considerations
//
//      KERNEL (writer)                    USERSPACE (reader)
//      ───────────────                    ──────────────────
//
//      // Update multiple fields
//      dcb.time_used.store(x, Relaxed);
//      dcb.time_remaining.store(y, Relaxed);
//      dcb.state.store(z, Release);  ──────────────────────┐
//                         │                                │
//                         │ Release ensures all            │
//                         │ prior writes visible           │
//                         ▼                                ▼
//                                         let state = dcb.state.load(Acquire);
//                                         // Acquire ensures we see
//                                         // all writes before the Release
//                                         let used = dcb.time_used.load(Relaxed);
//                                         let rem = dcb.time_remaining.load(Relaxed);
//
//      Protocol:
//      - Kernel does Release store on state LAST
//      - Userspace does Acquire load on state FIRST
//      - Then can safely read other fields with Relaxed

// ═══════════════════════════════════════════════════════════════════
// DCB PAGES MANAGER
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DcbError {
    TooManyPages,
    PageNotAllocated,
    NoFreeDomains,
    NotAllocated,
    InvalidDomainId,
}

/// Manager for all DCB pages in the system.
///
/// Provides:
/// - Kernel-side mutable access for state updates
/// - Physical addresses for user-space mapping
/// - Domain ID allocation
pub struct DcbPages {
    /// Array of DCB pages (kernel virtual addresses)
    pages: [Option<&'static mut DcbPage>; Self::MAX_PAGES],
    /// Physical addresses of each page (for user mapping)
    phys_addrs: [Option<PhysAddr>; Self::MAX_PAGES], // TODO: Option<NonNull<PhysAddr>>
    /// Number of allocated pages
    num_pages: usize,
    /// Next domain ID to allocate
    next_domain_id: u32,
    /// Bitmap of allocated domain IDs
    allocated: [u64; Self::MAX_DOMAINS / 64],
}

impl DcbPages {
    /// Maximum number of DCB pages (supports up to 8192 domains)
    pub const MAX_PAGES: usize = 256;
    /// Maximum domains (256 pages × 32 DCBs/page)
    pub const MAX_DOMAINS: usize = Self::MAX_PAGES * DcbPage::DCBS_PER_PAGE as usize;

    /// Well-known user-space base address for DCB mapping
    /// This is mapped read-only into all domains
    pub const USER_BASE: VirtAddr = VirtAddr::new_unchecked(0x0000_7FFF_FE00_0000);

    /// Create empty DCB pages manager
    pub const fn new() -> Self {
        Self {
            pages: [const { None }; Self::MAX_PAGES],
            phys_addrs: [const { None }; Self::MAX_PAGES],
            num_pages: 0,
            next_domain_id: 0,
            allocated: [0; Self::MAX_DOMAINS / 64],
        }
    }

    /// Add a new DCB page (called during kernel init)
    ///
    /// # Safety
    /// - `page` must be valid, aligned, and not aliased
    /// - `phys_addr` must be the correct physical address
    pub unsafe fn add_page(
        &mut self,
        page: *mut DcbPage,
        phys_addr: PhysAddr,
    ) -> Result<usize, DcbError> {
        if self.num_pages >= Self::MAX_PAGES {
            return Err(DcbError::TooManyPages);
        }

        let idx = self.num_pages;
        self.pages[idx] = unsafe { Some(&mut *page) };
        self.phys_addrs[idx] = Some(phys_addr);
        self.num_pages += 1;

        Ok(idx)
    }

    /// Allocate a new domain ID and initialize its DCB
    pub fn allocate_domain(&mut self, scheduler_id: DomainId) -> Result<DomainId, DcbError> {
        // Find free slot
        let id = self.find_free_slot()?;

        // Mark as allocated
        let word = id as usize / 64;
        let bit = id as usize % 64;
        self.allocated[word] |= 1 << bit;

        // Initialize DCB
        let domain_id = DomainId(id);
        let dcb = self.get_mut(domain_id).ok_or(DcbError::PageNotAllocated)?;
        *dcb = DomainControlBlock::new(domain_id, scheduler_id);

        Ok(domain_id)
    }

    /// Release a domain ID
    pub fn release_domain(&mut self, id: DomainId) -> Result<(), DcbError> {
        let word = id.0 as usize / 64;
        let bit = id.0 as usize % 64;

        if self.allocated[word] & (1 << bit) == 0 {
            return Err(DcbError::NotAllocated);
        }

        // Mark as free
        self.allocated[word] &= !(1 << bit);

        // Clear DCB
        if let Some(dcb) = self.get_mut(id) {
            dcb.state
                .store(DomainState::Inactive as u32, Ordering::Release);
            dcb.id = DomainId::INVALID;
        }

        Ok(())
    }

    /// Get a DCB by domain ID (immutable)
    #[inline]
    pub fn get(&self, id: DomainId) -> Option<&DomainControlBlock> {
        let page_idx = id.page_index();
        let slot = id.slot_in_page();

        self.pages.get(page_idx)?.as_ref()?.get(slot)
    }

    /// Get a DCB by domain ID (mutable) - kernel only
    #[inline]
    pub fn get_mut(&mut self, id: DomainId) -> Option<&mut DomainControlBlock> {
        let page_idx = id.page_index();
        let slot = id.slot_in_page();

        self.pages.get_mut(page_idx)?.as_mut()?.get_mut(slot)
    }

    /// Get physical address of a DCB page (for user mapping)
    pub fn page_phys_addr(&self, page_idx: usize) -> Option<PhysAddr> {
        self.phys_addrs.get(page_idx).copied().flatten()
    }

    /// Get user-space virtual address for a domain's DCB
    pub fn user_addr(&self, id: DomainId) -> VirtAddr {
        VirtAddr::new(Self::USER_BASE.as_u64() + (id.0 as u64 * 128))
    }

    /// Iterate over all allocated domains
    pub fn iter_allocated(&self) -> impl Iterator<Item = DomainId> + '_ {
        self.allocated
            .iter()
            .enumerate()
            .flat_map(|(word_idx, &word)| {
                (0..64).filter_map(move |bit| {
                    if word & (1 << bit) != 0 {
                        Some(DomainId((word_idx * 64 + bit) as u32))
                    } else {
                        None
                    }
                })
            })
    }

    /// Number of allocated domains
    pub fn num_allocated(&self) -> usize {
        self.allocated.iter().map(|w| w.count_ones() as usize).sum()
    }

    fn find_free_slot(&self) -> Result<u32, DcbError> {
        let max_id = (self.num_pages * DcbPage::DCBS_PER_PAGE as usize) as u32;

        for (word_idx, &word) in self.allocated.iter().enumerate() {
            if word != !0 {
                let bit = word.trailing_ones();
                let id = (word_idx as u32 * 64) + bit;
                if id < max_id {
                    return Ok(id);
                }
            }
        }

        Err(DcbError::NoFreeDomains)
    }
}
