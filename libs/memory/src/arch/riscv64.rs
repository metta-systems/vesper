use {
    crate::arch_trait::{EntryKind, LevelCapabilities, TranslationArch},
    libaddress::{PhysAddr, VirtAddr},
    libmapping::{AccessPermissions, AttributeFields, MemAttributes},
};

// ---------------------------------------------------------------------------
// RISC-V Sv48 page table entry bit layout (4-level, 4KiB pages)
// ---------------------------------------------------------------------------
//
//  Sv48 virtual address split (48-bit, sign-extended from bit 47):
//
//    63-48    47-39    38-30    29-21    20-12    11-0
//    signx     VPN[3]   VPN[2]   VPN[1]   VPN[0]   off
//              L0       L1       L2       L3
//
//  Each level indexes 512 entries (9 bits). Table size = 512 * 8 = 4096 bytes.
//
//  Any non-leaf level can produce a superpage if R, W, or X bits are set:
//    L0 (VPN[3]): 512 GiB terapage (rare, usually table-only)
//    L1 (VPN[2]): 1 GiB gigapage
//    L2 (VPN[1]): 2 MiB megapage
//    L3 (VPN[0]): 4 KiB page (always leaf)
//
//  PTE format (64 bits):
//
//    [63:54]  reserved (must be 0)
//    [53:10]  PPN (Physical Page Number) — PPN[2] [53:28], PPN[1] [27:19], PPN[0] [18:10]
//    [9:8]    RSW (reserved for supervisor software)
//    [7]      D   (Dirty)
//    [6]      A   (Accessed)
//    [5]      G   (Global)
//    [4]      U   (User accessible)
//    [3]      X   (Executable)
//    [2]      W   (Writable)
//    [1]      R   (Readable)
//    [0]      V   (Valid)
//
//  Leaf vs non-leaf determination:
//    V=1 and R=0, W=0, X=0  =>  pointer to next-level table
//    V=1 and (R=1 or X=1)   =>  leaf page/superpage
//    V=0                     =>  invalid
//    W=1 and R=0             =>  reserved (invalid)

// PTE flag bits
const V: u64 = 1 << 0; // Valid
const R: u64 = 1 << 1; // Readable
const W: u64 = 1 << 2; // Writable
const X: u64 = 1 << 3; // Executable
const _U: u64 = 1 << 4; // User accessible (reserved for userspace support)
const _G: u64 = 1 << 5; // Global (reserved for global mappings)
const A: u64 = 1 << 6; // Accessed
const D: u64 = 1 << 7; // Dirty

// PPN extraction: PTE bits [53:10] contain the physical page number.
// Physical address = PPN << 12.
const PPN_SHIFT: u64 = 10;
const PPN_MASK: u64 = 0x003F_FFFF_FFFF_FC00; // bits [53:10]

// For superpages, lower PPN fields must be zero:
// 2MiB megapage (L2):  PPN[0] (bits [18:10]) must be 0
// 1GiB gigapage (L1):  PPN[0] and PPN[1] (bits [27:10]) must be 0
// 512GiB terapage (L0): PPN[0], PPN[1], PPN[2] partially (bits [37:10]) must be 0

// Block sizes
const SIZE_4K: usize = 4096;
const SIZE_2M: usize = 2 * 1024 * 1024;
const SIZE_1G: usize = 1024 * 1024 * 1024;
const SIZE_512G: usize = 512 * 1024 * 1024 * 1024;

/// Convert a PPN from a PTE into a physical address.
fn ppn_to_phys(raw: u64) -> u64 {
    ((raw & PPN_MASK) >> PPN_SHIFT) << 12
}

/// Convert a physical address into PPN bits positioned for a PTE.
fn phys_to_ppn(phys: u64) -> u64 {
    (phys >> 12) << PPN_SHIFT
}

/// Is this PTE a leaf (page/superpage)?
/// A valid entry is a leaf if any of R, W, X are set.
fn is_leaf(raw: u64) -> bool {
    raw & (R | W | X) != 0
}

/// RISC-V Sv48 4-level paging.
///
/// 4-level hierarchy with 512 entries per table, 4KiB alignment.
/// Supports superpages at L0 (512GiB), L1 (1GiB), L2 (2MiB), and
/// regular 4KiB pages at L3.
///
/// Unlike AArch64 and x86_64, RISC-V uses the same PTE format at every
/// level. The distinction between table pointer and leaf is purely based
/// on the R/W/X permission bits: if none are set, it's a table pointer.
#[allow(non_camel_case_types)]
pub struct RiscV_Sv48;

impl TranslationArch for RiscV_Sv48 {
    const NUM_LEVELS: usize = 4;

    fn entries_per_table(_level: usize) -> usize {
        512
    }

    fn table_alignment(_level: usize) -> usize {
        SIZE_4K
    }

    fn level_capabilities(level: usize) -> LevelCapabilities {
        match level {
            // L0: table pointer or 512GiB terapage (generally table-only in practice)
            0 => LevelCapabilities {
                supports_table_pointer: true,
                supports_block: true,
                block_size: SIZE_512G,
            },
            // L1: table pointer or 1GiB gigapage
            1 => LevelCapabilities {
                supports_table_pointer: true,
                supports_block: true,
                block_size: SIZE_1G,
            },
            // L2: table pointer or 2MiB megapage
            2 => LevelCapabilities {
                supports_table_pointer: true,
                supports_block: true,
                block_size: SIZE_2M,
            },
            // L3: 4KiB page only (leaf only)
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
            0 => 39, // VPN[3]
            1 => 30, // VPN[2]
            2 => 21, // VPN[1]
            3 => 12, // VPN[0]
            _ => return 0,
        };
        ((vaddr.as_u64() >> shift) & 0x1FF) as usize
    }

    fn decode_entry(raw: u64, _level: usize) -> EntryKind {
        if raw & V == 0 {
            return EntryKind::Invalid;
        }

        // W=1, R=0 is a reserved combination
        if raw & W != 0 && raw & R == 0 {
            return EntryKind::Invalid;
        }

        if is_leaf(raw) {
            EntryKind::Block(PhysAddr::new(ppn_to_phys(raw)))
        } else {
            EntryKind::Table(PhysAddr::new(ppn_to_phys(raw)))
        }
    }

    fn encode_table_entry(next_table_phys: PhysAddr, _level: usize) -> u64 {
        let addr = next_table_phys.as_u64();
        debug_assert!(addr & 0xFFF == 0, "Table address not 4K aligned");
        // V=1, R=W=X=0 => non-leaf (table pointer)
        phys_to_ppn(addr) | V
    }

    fn encode_block_entry(phys: PhysAddr, attr: AttributeFields, level: usize) -> u64 {
        let addr = phys.as_u64();

        // Verify superpage alignment: lower PPN fields must be zero
        match level {
            0 => debug_assert!(
                addr & (SIZE_512G as u64 - 1) == 0,
                "512GiB terapage not aligned"
            ),
            1 => debug_assert!(
                addr & (SIZE_1G as u64 - 1) == 0,
                "1GiB gigapage not aligned"
            ),
            2 => debug_assert!(
                addr & (SIZE_2M as u64 - 1) == 0,
                "2MiB megapage not aligned"
            ),
            _ => panic!("Use encode_page_entry for L3 leaf entries"),
        }

        phys_to_ppn(addr) | encode_attributes(attr) | V
    }

    fn encode_page_entry(phys: PhysAddr, attr: AttributeFields) -> u64 {
        let addr = phys.as_u64();
        debug_assert!(addr & 0xFFF == 0, "Page address not 4K aligned");
        phys_to_ppn(addr) | encode_attributes(attr) | V
    }

    fn output_address(raw: u64, _level: usize) -> PhysAddr {
        PhysAddr::new(ppn_to_phys(raw))
    }
}

/// Encode `AttributeFields` into RISC-V PTE permission/attribute bits.
///
/// RISC-V is simpler than ARM/x86 — no MAIR or PWT/PCD. Memory type
/// is controlled by the Svpbmt extension (bits [62:61]) when available,
/// but the base spec treats all memory as cacheable. For device memory
/// we rely on Svpbmt or PMA (Physical Memory Attributes) configured
/// elsewhere.
fn encode_attributes(attr: AttributeFields) -> u64 {
    let mut bits: u64 = 0;

    // Pre-set Accessed and Dirty to avoid page faults on first access.
    bits |= A | D;

    // RISC-V permission model: R, W, X bits directly.
    // At minimum, a leaf entry must have R or X set.
    match attr.acc_perms {
        AccessPermissions::ReadWrite => bits |= R | W,
        AccessPermissions::ReadOnly => bits |= R,
    }

    if attr.executable {
        bits |= X;
    }

    // Supervisor-only for now (U=0).
    // When userspace is implemented, set U based on context.

    // Memory type hints via Svpbmt (bits [62:61]) when the extension is
    // available. Without Svpbmt, PMAs define memory type and these bits
    // are reserved-zero.
    //
    // Svpbmt encoding:
    //   00 = PMA (default, use Physical Memory Attributes)
    //   01 = NC  (non-cacheable)
    //   10 = IO  (I/O, strongly ordered)
    //   11 = reserved
    match attr.mem_attributes {
        MemAttributes::CacheableDRAM => {
            // 00 = PMA default (cacheable for normal RAM)
        }
        MemAttributes::NonCacheableDRAM => {
            // 01 = NC
            bits |= 1 << 61;
        }
        MemAttributes::Device => {
            // 10 = IO
            bits |= 2 << 61;
        }
    }

    bits
}
