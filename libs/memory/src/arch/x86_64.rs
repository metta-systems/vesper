use {
    crate::arch_trait::{EntryKind, LevelCapabilities, TranslationArch},
    libaddress::{PhysAddr, VirtAddr},
    libmapping::{AccessPermissions, AttributeFields, MemAttributes},
};

// ---------------------------------------------------------------------------
// x86_64 page table entry bit layout (4-level, 4KiB pages)
// ---------------------------------------------------------------------------
//
//  Virtual address split (48-bit canonical):
//
//    63-48    47-39    38-30    29-21    20-12    11-0
//    signx     PML4     PDPT      PD       PT      off
//    (copy      L0       L1       L2       L3
//    of b47)
//
//  Each level indexes 512 entries (9 bits). Table size = 512 * 8 = 4096 bytes.
//
//  L0 (PML4):  table pointer only
//  L1 (PDPT):  table pointer or 1GiB page (if PS bit set)
//  L2 (PD):    table pointer or 2MiB page (if PS bit set)
//  L3 (PT):    4KiB page only
//
//  Common entry format:
//
//    [63]     NX  (No Execute)
//    [62:52]  available / reserved (depending on feature)
//    [51:12]  physical address (4K aligned; [51:M] must be 0 where M = MAXPHYADDR)
//    [11:9]   available
//    [8]      G   (Global, ignored in PML4E/PDPTE)
//    [7]      PS  (Page Size: 0=points to table, 1=maps large page; must be 0 in PML4E and PTE)
//    [6]      D   (Dirty, only on leaf entries)
//    [5]      A   (Accessed)
//    [4]      PCD (Page-level Cache Disable)
//    [3]      PWT (Page-level Write-Through)
//    [2]      U/S (User/Supervisor: 0=supervisor only, 1=user accessible)
//    [1]      R/W (Read/Write: 0=read-only, 1=writable)
//    [0]      P   (Present)
//
//  Large page (1GiB at L1, 2MiB at L2):
//    Same as above but PS=1, and address bits below the page size are
//    repurposed: [20:13] = PAT/reserved for 2MiB, [29:13] for 1GiB.

// Entry bit positions
const PRESENT: u64 = 1 << 0;
const WRITABLE: u64 = 1 << 1;
const USER_ACCESSIBLE: u64 = 1 << 2;
const WRITE_THROUGH: u64 = 1 << 3;
const CACHE_DISABLE: u64 = 1 << 4;
const ACCESSED: u64 = 1 << 5;
const DIRTY: u64 = 1 << 6;
const PAGE_SIZE: u64 = 1 << 7; // PS bit — large page at L1/L2
const NO_EXECUTE: u64 = 1 << 63;

// Address masks
const ADDR_MASK_4K: u64 = 0x000F_FFFF_FFFF_F000; // [51:12]
const ADDR_MASK_2M: u64 = 0x000F_FFFF_FFE0_0000; // [51:21]
const ADDR_MASK_1G: u64 = 0x000F_FFFF_C000_0000; // [51:30]

// Block sizes
const SIZE_4K: usize = 4096;
const SIZE_2M: usize = 2 * 1024 * 1024;
const SIZE_1G: usize = 1024 * 1024 * 1024;

/// x86_64 4-level paging with 4KiB pages.
///
/// 4-level hierarchy: PML4 (L0) -> PDPT (L1) -> PD (L2) -> PT (L3).
/// 512 entries per table, 4096-byte table size, 4096-byte alignment.
///
/// Supports 1GiB huge pages at L1 (PDPT) and 2MiB large pages at L2 (PD)
/// via the PS (Page Size) bit.
pub struct X86_64_4K;

impl TranslationArch for X86_64_4K {
    const NUM_LEVELS: usize = 4;

    fn entries_per_table(_level: usize) -> usize {
        512
    }

    fn table_alignment(_level: usize) -> usize {
        SIZE_4K
    }

    fn level_capabilities(level: usize) -> LevelCapabilities {
        match level {
            // PML4: table pointer only (no large pages)
            0 => LevelCapabilities {
                supports_table_pointer: true,
                supports_block: false,
                block_size: 0,
            },
            // PDPT: table pointer or 1GiB huge page
            1 => LevelCapabilities {
                supports_table_pointer: true,
                supports_block: true,
                block_size: SIZE_1G,
            },
            // PD: table pointer or 2MiB large page
            2 => LevelCapabilities {
                supports_table_pointer: true,
                supports_block: true,
                block_size: SIZE_2M,
            },
            // PT: 4KiB page only
            3 => LevelCapabilities {
                supports_table_pointer: false,
                supports_block: true,
                block_size: SIZE_4K,
            },
            _ => LevelCapabilities {
                supports_table_pointer: false,
                supports_block: false,
                block_size: 0,
            },
        }
    }

    fn index_from_vaddr(vaddr: VirtAddr, level: usize) -> usize {
        let shift = match level {
            0 => 39, // PML4 index
            1 => 30, // PDPT index
            2 => 21, // PD index
            3 => 12, // PT index
            _ => return 0,
        };
        ((vaddr.as_u64() >> shift) & 0x1FF) as usize
    }

    fn decode_entry(raw: u64, level: usize) -> EntryKind {
        if raw & PRESENT == 0 {
            return EntryKind::Invalid;
        }

        match level {
            // PML4: always a table pointer (PS must be 0)
            0 => EntryKind::Table(PhysAddr::new(raw & ADDR_MASK_4K)),
            // PDPT: PS=1 means 1GiB page, PS=0 means table pointer
            1 => {
                if raw & PAGE_SIZE != 0 {
                    EntryKind::Block(PhysAddr::new(raw & ADDR_MASK_1G))
                } else {
                    EntryKind::Table(PhysAddr::new(raw & ADDR_MASK_4K))
                }
            }
            // PD: PS=1 means 2MiB page, PS=0 means table pointer
            2 => {
                if raw & PAGE_SIZE != 0 {
                    EntryKind::Block(PhysAddr::new(raw & ADDR_MASK_2M))
                } else {
                    EntryKind::Table(PhysAddr::new(raw & ADDR_MASK_4K))
                }
            }
            // PT: always a 4KiB page (PS is ignored / must be 0)
            3 => EntryKind::Block(PhysAddr::new(raw & ADDR_MASK_4K)),
            _ => EntryKind::Invalid,
        }
    }

    fn encode_table_entry(next_table_phys: PhysAddr, _level: usize) -> u64 {
        let addr = next_table_phys.as_u64();
        debug_assert!(addr & !ADDR_MASK_4K == 0, "Table address not 4K aligned");
        // Table entries: P=1, R/W=1, U/S=1 (permissive — leaf entries restrict further)
        (addr & ADDR_MASK_4K) | USER_ACCESSIBLE | WRITABLE | PRESENT
    }

    fn encode_block_entry(phys: PhysAddr, attr: AttributeFields, level: usize) -> u64 {
        let addr = phys.as_u64();
        let addr_mask = match level {
            1 => ADDR_MASK_1G,
            2 => ADDR_MASK_2M,
            _ => panic!("Large page entries only valid at L1 (1GiB) and L2 (2MiB)"),
        };
        debug_assert!(
            addr & !addr_mask == 0,
            "Block address not aligned for level"
        );
        // PS=1 for large pages at L1/L2
        (addr & addr_mask) | PAGE_SIZE | encode_attributes(attr) | PRESENT
    }

    fn encode_page_entry(phys: PhysAddr, attr: AttributeFields) -> u64 {
        let addr = phys.as_u64();
        debug_assert!(addr & !ADDR_MASK_4K == 0, "Page address not 4K aligned");
        // PT entries: no PS bit, just P=1
        (addr & ADDR_MASK_4K) | encode_attributes(attr) | PRESENT
    }

    fn output_address(raw: u64, level: usize) -> PhysAddr {
        let mask = match level {
            0 => ADDR_MASK_4K,
            1 => {
                if raw & PAGE_SIZE != 0 {
                    ADDR_MASK_1G
                } else {
                    ADDR_MASK_4K
                }
            }
            2 => {
                if raw & PAGE_SIZE != 0 {
                    ADDR_MASK_2M
                } else {
                    ADDR_MASK_4K
                }
            }
            3 => ADDR_MASK_4K,
            _ => 0,
        };
        PhysAddr::new(raw & mask)
    }
}

/// Encode `AttributeFields` into the x86_64 PTE flag bits.
///
/// x86_64 uses a different model from ARM:
/// - No MAIR — caching is controlled by PWT and PCD bits directly
/// - Permissions are R/W (bit 1) and U/S (bit 2)
/// - Execute control is NX (bit 63), opt-out rather than opt-in
fn encode_attributes(attr: AttributeFields) -> u64 {
    let mut bits: u64 = 0;

    // Accessed and Dirty — pre-set to avoid faults on first access.
    // The kernel can clear these for tracking if needed.
    bits |= ACCESSED | DIRTY;

    // Memory type via PWT/PCD
    match attr.mem_attributes {
        MemAttributes::CacheableDRAM => {
            // PWT=0, PCD=0: write-back cacheable
        }
        MemAttributes::NonCacheableDRAM => {
            // PWT=1, PCD=1: uncacheable (UC)
            // For true WC you'd use PAT, but UC is the safe no_std default.
            bits |= WRITE_THROUGH | CACHE_DISABLE;
        }
        MemAttributes::Device => {
            // PWT=0, PCD=1: uncacheable, strong ordering
            bits |= CACHE_DISABLE;
        }
    }

    // Access permissions — x86_64 is permissive by default (read always allowed).
    // R/W bit: 0 = read-only, 1 = read-write
    if let AccessPermissions::ReadWrite = attr.acc_perms {
        bits |= WRITABLE;
    }

    // Execute-never via NX bit
    if !attr.executable {
        bits |= NO_EXECUTE;
    }

    // Supervisor-only for now (U/S = 0)
    // When userspace is implemented, set USER_ACCESSIBLE based on context.

    bits
}
