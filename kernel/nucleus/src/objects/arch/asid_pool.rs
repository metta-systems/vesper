use {
    crate::objects::{NucleusObject, arch_objects::AsidPoolObject},
    libobject::ObjectType,
};

/// Kernel state of one `ASIDPool`: a bitmap of allocated ASIDs.
///
/// A pool covers 512 ASIDs of the 16-bit hardware ASID space. ASID 0 is
/// reserved: the kernel's own boot translation context runs with TTBR0's ASID
/// field zero, so a pool never hands it out. The boot pool is the only pool
/// today (boot-provided, not Retype-creatable — ASIDs are a hardware
/// namespace, not memory-backed); partitioning the 16-bit space across
/// multiple pools is future work recorded with the ASID contract.
pub struct AArch64ASIDPool {
    /// Allocation bitmap; bit `n` set means ASID `n` is taken.
    allocated: [u64; Self::WORDS],
}

impl AArch64ASIDPool {
    /// ASIDs managed by one pool: 8 words × 64 bits.
    pub const ASIDS_PER_POOL: usize = 512;
    const WORDS: usize = Self::ASIDS_PER_POOL / 64;

    /// A fresh pool with ASID 0 reserved (the kernel's boot context).
    pub fn new() -> Self {
        let mut allocated = [0; Self::WORDS];
        allocated[0] = 1; // ASID 0 is reserved for the kernel's boot context.
        Self { allocated }
    }
}

impl AsidPoolObject for AArch64ASIDPool {
    fn allocate(&mut self) -> Option<u16> {
        for (word_index, word) in self.allocated.iter_mut().enumerate() {
            if *word != !0 {
                let bit = word.trailing_ones();
                *word |= 1 << bit;
                return u16::try_from(word_index * 64 + bit as usize).ok();
            }
        }
        None
    }

    fn release(&mut self, asid: u16) {
        // ASID 0 is the kernel's own reserved boot context and is never
        // released; a retirement carrying it is a kernel bookkeeping bug.
        debug_assert_ne!(asid, 0);
        let word = usize::from(asid) / 64;
        let bit = usize::from(asid) % 64;
        if let Some(word) = self.allocated.get_mut(word) {
            *word &= !(1 << bit);
        }
    }
}

impl NucleusObject for AArch64ASIDPool {
    const TYPE: ObjectType = ObjectType::ASID_POOL;
    const POOL: crate::objects::access::PoolTag = crate::objects::access::PoolTag::ASIDPool;
}
