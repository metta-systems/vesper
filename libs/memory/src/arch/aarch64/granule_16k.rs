use {
    super::common::*,
    crate::arch_trait::{EntryKind, LevelCapabilities, TranslationArch},
    libaddress::{PhysAddr, VirtAddr},
    libmapping::AttributeFields,
};

// ---------------------------------------------------------------------------
// AArch64 Stage 1, 16KiB granule
// ---------------------------------------------------------------------------
//
//  Virtual address split (48-bit VA):
//
//    63-48    47     46-36     35-25     24-14     13-0
//    signx    L0      L1        L2        L3       off
//             1bit   11bits    11bits    11bits    14bits
//
//  L0: 2 entries, table pointer only. Table size = 16 bytes.
//  L1: 2048 entries, table pointer only.
//  L2: 2048 entries, table pointer or 32MiB block.
//  L3: 2048 entries, 16KiB page.
//
//  All tables are 16KiB-aligned (including L0, even though it is only 16 bytes).

// Address masks (16K granule specific)
const L2_BLOCK_ADDR_MASK: u64 = 0x0000_FFFF_FE00_0000; // [47:25]
const L3_PAGE_ADDR_MASK: u64 = 0x0000_FFFF_FFFF_C000; // [47:14]

// Block sizes
const SIZE_16K: usize = 16 * 1024;
const SIZE_32M: usize = 32 * 1024 * 1024;

/// AArch64 Stage 1 translation with 16KiB granule.
///
/// 4-level hierarchy: L0 (2 entries) -> L1 (2048) -> L2 (2048) -> L3 (2048).
/// L0 table is only 16 bytes but requires 16KiB alignment.
pub struct Aarch64_16K;

impl TranslationArch for Aarch64_16K {
    const NUM_LEVELS: usize = 4;

    fn entries_per_table(level: usize) -> usize {
        match level {
            0 => 2,
            1..=3 => 2048,
            _ => 0,
        }
    }

    fn table_alignment(_level: usize) -> usize {
        SIZE_16K
    }

    fn level_capabilities(level: usize) -> LevelCapabilities {
        match level {
            0 | 1 => LevelCapabilities {
                supports_table_pointer: true,
                supports_block: false,
                block_size: 0,
            },
            2 => LevelCapabilities {
                supports_table_pointer: true,
                supports_block: true,
                block_size: SIZE_32M,
            },
            3 => LevelCapabilities {
                supports_table_pointer: false,
                supports_block: true,
                block_size: SIZE_16K,
            },
            _ => LevelCapabilities {
                supports_table_pointer: false,
                supports_block: false,
                block_size: 0,
            },
        }
    }

    fn index_from_vaddr(vaddr: VirtAddr, level: usize) -> usize {
        let (shift, mask) = match level {
            0 => (47, 0x1),
            1 => (36, 0x7FF),
            2 => (25, 0x7FF),
            3 => (14, 0x7FF),
            _ => return 0,
        };
        ((vaddr.as_u64() >> shift) & mask) as usize
    }

    fn decode_entry(raw: u64, level: usize) -> EntryKind {
        if raw & VALID_BIT == 0 {
            return EntryKind::Invalid;
        }

        match level {
            // L0, L1: TYPE=1 means table, TYPE=0 is reserved/invalid
            0 | 1 => {
                if raw & TYPE_BIT != 0 {
                    EntryKind::Table(PhysAddr::new(raw & TABLE_ADDR_MASK))
                } else {
                    EntryKind::Invalid
                }
            }
            // L2: TYPE=1 means table, TYPE=0 means 32MiB block
            2 => {
                if raw & TYPE_BIT != 0 {
                    EntryKind::Table(PhysAddr::new(raw & TABLE_ADDR_MASK))
                } else {
                    EntryKind::Block(PhysAddr::new(raw & L2_BLOCK_ADDR_MASK))
                }
            }
            // L3: TYPE=1 means valid page, TYPE=0 is reserved/invalid
            3 => {
                if raw & TYPE_BIT != 0 {
                    EntryKind::Block(PhysAddr::new(raw & L3_PAGE_ADDR_MASK))
                } else {
                    EntryKind::Invalid
                }
            }
            _ => EntryKind::Invalid,
        }
    }

    fn encode_table_entry(next_table_phys: PhysAddr, _level: usize) -> u64 {
        let addr = next_table_phys.as_u64();
        debug_assert!(
            addr & !TABLE_ADDR_MASK == 0,
            "Table address not properly aligned"
        );
        (addr & TABLE_ADDR_MASK) | TYPE_BIT | VALID_BIT
    }

    fn encode_block_entry(phys: PhysAddr, attr: AttributeFields, level: usize) -> u64 {
        let addr = phys.as_u64();
        let addr_mask = match level {
            2 => L2_BLOCK_ADDR_MASK,
            _ => panic!("Block entries only valid at L2 for 16K granule"),
        };
        debug_assert!(
            addr & !addr_mask == 0,
            "Block address not aligned for level"
        );
        (addr & addr_mask) | encode_attributes(attr) | AF_BIT | VALID_BIT
    }

    fn encode_page_entry(phys: PhysAddr, attr: AttributeFields) -> u64 {
        let addr = phys.as_u64();
        debug_assert!(
            addr & !L3_PAGE_ADDR_MASK == 0,
            "Page address not 16K aligned"
        );
        (addr & L3_PAGE_ADDR_MASK) | encode_attributes(attr) | AF_BIT | TYPE_BIT | VALID_BIT
    }

    fn output_address(raw: u64, level: usize) -> PhysAddr {
        let mask = match level {
            0 | 1 => TABLE_ADDR_MASK,
            2 => {
                if raw & TYPE_BIT != 0 {
                    TABLE_ADDR_MASK
                } else {
                    L2_BLOCK_ADDR_MASK
                }
            }
            3 => L3_PAGE_ADDR_MASK,
            _ => 0,
        };
        PhysAddr::new(raw & mask)
    }
}
