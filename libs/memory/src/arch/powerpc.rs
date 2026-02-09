use {
    crate::arch_trait::{EntryKind, LevelCapabilities, TranslationArch},
    libaddress::{PhysAddr, VirtAddr},
    libmapping::{AccessPermissions, AttributeFields, MemAttributes},
};

// ---------------------------------------------------------------------------
// PowerPC 970MP Hashed Page Table (HPT) entry layout
// ---------------------------------------------------------------------------
//
//  The 970MP uses a hashed page table (HPT) rather than hierarchical page
//  tables. Translation is a two-step process:
//
//  1. SLB (Segment Lookaside Buffer) translates the effective address (EA)
//     into a virtual address (VA) by providing a VSID.
//
//  2. The HPT maps the VA to a real (physical) address via hash lookup.
//
//  The HPT is a flat array of PTEGs (Page Table Entry Groups). Each PTEG
//  contains 8 PTEs. Each PTE is 16 bytes (two 64-bit words).
//
//  PTEG selection:
//    primary_hash   = (VSID ^ page_index) & htab_mask
//    secondary_hash = ~primary_hash & htab_mask
//
//  PTE format (16 bytes = 2 × u64):
//
//  pte_hi (word 0):
//    [63]     V      — Valid
//    [62:7]   AVPN   — Abbreviated Virtual Page Number
//    [6:2]   (reserved, software use)
//    [1]      H      — Hash function identifier (0=primary, 1=secondary)
//    [0]      (reserved)
//
//  For large pages (16MB):
//    [63]     V      — Valid
//    [62:7]   AVPN   — but bits [22:12] of the VA are in pte_hi[24:14]
//    [3]      L      — Large page (1 = 16MB)
//    [1]      H      — Hash function identifier
//
//  pte_lo (word 1):
//    [63:12]  RPGN   — Real Page Group Number (physical page frame)
//    [11:9]  (reserved)
//    [8]      R      — Referenced (set by hardware)
//    [7]      C      — Changed (set by hardware)
//    [6:3]    WIMG   — Memory/cache control
//                      W = Write-through
//                      I = Cache-inhibited
//                      M = Memory coherence required
//                      G = Guarded (no speculative access)
//    [2]      N      — No-execute
//    [1:0]    PP     — Page protection
//                      00 = no access (supervisor only)
//                      01 = supervisor read/write
//                      10 = supervisor read/write, user read/write
//                      11 = supervisor read-only, user read-only

// --- pte_hi bits ---
const PTE_HI_VALID: u64 = 1 << 63;
const PTE_HI_HASH: u64 = 1 << 1;
const PTE_HI_LARGE: u64 = 1 << 3;

// AVPN occupies bits [62:7] of pte_hi.
// For a 4KB page, AVPN = VA[77:23] (the upper bits of the virtual page number).
const PTE_HI_AVPN_MASK: u64 = 0x7FFF_FFFF_FFFF_FF80;

// --- pte_lo bits ---
const PTE_LO_RPGN_MASK: u64 = 0xFFFF_FFFF_FFFF_F000; // bits [63:12]
const PTE_LO_REF: u64 = 1 << 8;
const PTE_LO_CHG: u64 = 1 << 7;
const PTE_LO_NOEXEC: u64 = 1 << 2;

// WIMG field: bits [6:3]
const PTE_LO_WIMG_SHIFT: u64 = 3;
const _PTE_LO_WIMG_MASK: u64 = 0xF << PTE_LO_WIMG_SHIFT;

// WIMG bit values (shifted)
const WIMG_M: u64 = 0b0010 << PTE_LO_WIMG_SHIFT; // Memory coherence
const WIMG_IG: u64 = 0b0101 << PTE_LO_WIMG_SHIFT; // Cache-inhibited + Guarded

// PP field: bits [1:0]
const _PP_NO_ACCESS: u64 = 0b00;
const PP_RW_SUPER: u64 = 0b01;
const _PP_RW_ALL: u64 = 0b10;
const PP_RO_ALL: u64 = 0b11;

// Page sizes
const SIZE_4K: usize = 4096;
const _SIZE_16M: usize = 16 * 1024 * 1024;

// PTEs per PTEG
const PTES_PER_PTEG: usize = 8;

/// PowerPC 970MP hashed page table with 4KiB base page size.
///
/// The 970MP uses a fundamentally different translation scheme from
/// hierarchical page tables. Instead of a multi-level tree, it uses:
///
/// - **SLB** (Segment Lookaside Buffer): hardware-managed register file
///   that maps effective address segments to VSIDs. Managed separately
///   from this module.
///
/// - **HPT** (Hashed Page Table): a flat hash table of PTEGs. Each PTEG
///   contains 8 PTEs (16 bytes each). The hash is computed from the VSID
///   and virtual page number.
///
/// Since the HPT is a single flat structure, `NUM_LEVELS = 1` and
/// `ENTRY_WIDTH = 2` (each PTE is two u64 words: pte_hi and pte_lo).
///
/// Page sizes: 4KiB (base) and 16MiB (large, indicated by the L bit).
#[allow(non_camel_case_types)]
pub struct PowerPC_970;

impl PowerPC_970 {
    /// Number of PTEGs for a given HPT size in bytes.
    /// HPT size must be a power of 2, minimum 256KB (2^18).
    pub fn ptegs_for_htab_size(htab_size_bytes: usize) -> usize {
        htab_size_bytes / (PTES_PER_PTEG * 16)
    }

    /// Compute the htab_mask from the HPT size in bytes.
    /// This masks the hash to select a PTEG within the table.
    pub fn htab_mask(htab_size_bytes: usize) -> u64 {
        (Self::ptegs_for_htab_size(htab_size_bytes) - 1) as u64
    }

    /// Build the AVPN field for pte_hi from a VSID and VA page index.
    ///
    /// For 4KB pages: AVPN = VSID[36:0] << 23 | page_index[15:11]
    /// (bits of the VA that are NOT part of the hash).
    pub fn build_avpn(vsid: u64, va_page_index: u64) -> u64 {
        // AVPN goes in bits [62:7] of pte_hi.
        // The AVPN encodes VSID and the upper bits of the page index.
        let avpn = (vsid << 12) | (va_page_index >> 4);
        (avpn << 7) & PTE_HI_AVPN_MASK
    }

    /// Extract the page index from a virtual address for 4KB pages.
    pub fn page_index_4k(vaddr: VirtAddr) -> u64 {
        (vaddr.as_u64() >> 12) & 0xFFFF
    }

    /// Extract the page index from a virtual address for 16MB large pages.
    pub fn page_index_16m(vaddr: VirtAddr) -> u64 {
        (vaddr.as_u64() >> 24) & 0xFF
    }
}

impl TranslationArch for PowerPC_970 {
    const NUM_LEVELS: usize = 1;
    const ENTRY_WIDTH: usize = 2; // 16-byte PTEs: [pte_hi, pte_lo]
    const HASHED: bool = true;

    fn entries_per_table(_level: usize) -> usize {
        // The actual number of entries depends on the HPT size, which is
        // runtime-configurable. For trait purposes we return a nominal
        // value. Real table creation uses the runtime HPT size.
        //
        // A minimum HPT is 256KB = 16384 entries (2048 PTEGs × 8 PTEs).
        // Callers should use PowerPC_970::ptegs_for_htab_size() and
        // construct tables with the appropriate size.
        2048 * PTES_PER_PTEG
    }

    fn table_alignment(_level: usize) -> usize {
        // HPT must be naturally aligned to its size. The minimum is 256KB.
        // In practice the kernel allocates a power-of-2 aligned region.
        256 * 1024
    }

    fn level_capabilities(level: usize) -> LevelCapabilities {
        match level {
            0 => LevelCapabilities {
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

    fn index_from_vaddr(_vaddr: VirtAddr, _level: usize) -> usize {
        // HPT does not use hierarchical index extraction.
        // Use hash_primary/hash_secondary instead.
        0
    }

    // -- Single-u64 methods are not meaningful for HPT --

    fn decode_entry(_raw: u64, _level: usize) -> EntryKind {
        // Use decode_entry_wide for 16-byte PTEs.
        EntryKind::Invalid
    }

    fn encode_table_entry(_next_table_phys: PhysAddr, _level: usize) -> u64 {
        // HPT has no table pointers.
        0
    }

    fn encode_block_entry(_phys: PhysAddr, _attr: AttributeFields, _level: usize) -> u64 {
        // Use encode_page_entry_wide for HPT entries.
        0
    }

    fn encode_page_entry(_phys: PhysAddr, _attr: AttributeFields) -> u64 {
        // Use encode_page_entry_wide for HPT entries.
        0
    }

    fn output_address(_raw: u64, _level: usize) -> PhysAddr {
        // Use output_address_wide for HPT entries.
        PhysAddr::new(0)
    }

    // -- Wide entry methods (the real API for HPT) --

    fn decode_entry_wide(raw: &[u64], _level: usize) -> EntryKind {
        debug_assert_eq!(raw.len(), 2);
        let pte_hi = raw[0];
        let pte_lo = raw[1];

        if pte_hi & PTE_HI_VALID == 0 {
            return EntryKind::Invalid;
        }

        let rpgn = pte_lo & PTE_LO_RPGN_MASK;
        EntryKind::Block(PhysAddr::new(rpgn))
    }

    fn encode_page_entry_wide(phys: PhysAddr, attr: AttributeFields, buf: &mut [u64]) {
        debug_assert_eq!(buf.len(), 2);
        let addr = phys.as_u64();
        debug_assert!(addr & 0xFFF == 0, "Physical address not 4K aligned");

        // Build pte_lo: RPGN + WIMG + PP + N + R + C pre-set
        let mut pte_lo = addr & PTE_LO_RPGN_MASK;
        pte_lo |= encode_wimg(attr.mem_attributes);
        pte_lo |= encode_pp(attr.acc_perms);
        if attr.execute_never {
            pte_lo |= PTE_LO_NOEXEC;
        }
        // Pre-set Referenced and Changed to avoid software faults.
        pte_lo |= PTE_LO_REF | PTE_LO_CHG;

        // Build pte_hi: Valid bit set. AVPN and H bit must be filled in
        // by the caller (they depend on VSID and hash group), so we just
        // set the valid bit here. The caller uses write_raw_wide to
        // compose the final entry with AVPN.
        let pte_hi = PTE_HI_VALID;

        buf[0] = pte_hi;
        buf[1] = pte_lo;
    }

    fn output_address_wide(raw: &[u64], _level: usize) -> PhysAddr {
        debug_assert_eq!(raw.len(), 2);
        let pte_lo = raw[1];
        PhysAddr::new(pte_lo & PTE_LO_RPGN_MASK)
    }

    // -- Hash-based lookup --

    fn hash_primary(vaddr: VirtAddr, vsid: u64, htab_mask: u64) -> usize {
        // Primary hash = (VSID ^ page_index) & htab_mask
        let page_index = PowerPC_970::page_index_4k(vaddr);
        ((vsid ^ page_index) & htab_mask) as usize
    }

    fn hash_secondary(primary_hash: usize, htab_mask: u64) -> usize {
        // Secondary hash = ~primary_hash & htab_mask
        (!(primary_hash as u64) & htab_mask) as usize
    }
}

/// Encode memory attributes into WIMG bits for pte_lo.
fn encode_wimg(mem_attr: MemAttributes) -> u64 {
    match mem_attr {
        // Normal cacheable memory: M=1 (coherence required for SMP)
        MemAttributes::CacheableDRAM => WIMG_M,
        // Non-cacheable memory: W=0, I=1, M=0, G=0
        MemAttributes::NonCacheableDRAM => 0b0100 << PTE_LO_WIMG_SHIFT,
        // Device/MMIO: I=1, G=1 (cache-inhibited + guarded)
        MemAttributes::Device => WIMG_IG,
    }
}

/// Encode access permissions into PP bits for pte_lo.
fn encode_pp(acc_perms: AccessPermissions) -> u64 {
    match acc_perms {
        // Supervisor read/write (user no access for now)
        AccessPermissions::ReadWrite => PP_RW_SUPER,
        // Read-only for all
        AccessPermissions::ReadOnly => PP_RO_ALL,
    }
}

/// Helper for constructing a complete HPT PTE with AVPN and hash info.
///
/// `encode_page_entry_wide` sets the valid bit and physical attributes but
/// leaves AVPN and H zeroed. This function builds the complete pte_hi
/// word with the full AVPN, hash-group indicator, and optional large-page bit.
///
/// Typical usage: call `encode_page_entry_wide` to get the base [pte_hi, pte_lo],
/// then OR the result of this function into `buf[0]` to set AVPN and H.
#[allow(dead_code)]
pub fn build_complete_pte_hi(vsid: u64, vaddr: VirtAddr, secondary: bool, large_page: bool) -> u64 {
    let page_index = if large_page {
        PowerPC_970::page_index_16m(vaddr)
    } else {
        PowerPC_970::page_index_4k(vaddr)
    };

    let mut pte_hi = PTE_HI_VALID;
    pte_hi |= PowerPC_970::build_avpn(vsid, page_index);

    if secondary {
        pte_hi |= PTE_HI_HASH;
    }
    if large_page {
        pte_hi |= PTE_HI_LARGE;
    }

    pte_hi
}
