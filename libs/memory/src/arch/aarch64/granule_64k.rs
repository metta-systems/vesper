use {
    super::common::*,
    crate::arch_trait::{EntryKind, LevelCapabilities, TranslationArch},
    libaddress::{PhysAddr, VirtAddr},
    libmapping::AttributeFields,
};

// ---------------------------------------------------------------------------
// AArch64 Stage 1, 64KiB granule
// ---------------------------------------------------------------------------
//
//  Virtual address split (48-bit VA, no LPA2):
//
//    63-48    47-42    41-29    28-16    15-0
//    signx     L0       L1       L2      off
//             6bits   13bits   13bits   16bits
//
//  Our level 0 corresponds to ARM's "Level 1" — we always number from
//  the root. ARM skips Level 0 entirely for 64K granule.
//
//  L0: 64 entries, table pointer only. Table size = 512 bytes.
//  L1: 8192 entries, table pointer or 512MiB block.
//  L2: 8192 entries, 64KiB page.
//
//  All tables require 64KiB alignment (including L0, even though it
//  is only 512 bytes).

// Address masks (64K granule specific)
const L1_BLOCK_ADDR_MASK: u64 = 0x0000_FFFE_0000_0000; // [47:29]
const L2_PAGE_ADDR_MASK: u64 = 0x0000_FFFF_FFFF_0000; // [47:16]

// Block sizes
const SIZE_64K: usize = 64 * 1024;
const SIZE_512M: usize = 512 * 1024 * 1024;

/// `AArch64` Stage 1 translation with 64KiB granule.
///
/// 3-level hierarchy: L0 (64 entries) -> L1 (8192) -> L2 (8192).
/// Our level numbering is 0-based from root; ARM calls these Level 1/2/3.
pub struct Aarch64_64K;

impl TranslationArch for Aarch64_64K {
    const NUM_LEVELS: usize = 3;

    fn entries_per_table(level: usize) -> usize {
        match level {
            0 => 64,
            1 | 2 => 8192,
            _ => 0,
        }
    }

    fn table_alignment(_level: usize) -> usize {
        SIZE_64K
    }

    fn level_capabilities(level: usize) -> LevelCapabilities {
        match level {
            0 => LevelCapabilities {
                supports_table_pointer: true,
                supports_block: false,
                block_size: 0,
            },
            1 => LevelCapabilities {
                supports_table_pointer: true,
                supports_block: true,
                block_size: SIZE_512M,
            },
            2 => LevelCapabilities {
                supports_table_pointer: false,
                supports_block: true,
                block_size: SIZE_64K,
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
            0 => (42, 0x3F),
            1 => (29, 0x1FFF),
            2 => (16, 0x1FFF),
            _ => return 0,
        };
        usize::try_from((vaddr.as_u64() >> shift) & mask).unwrap()
    }

    fn decode_entry(raw: u64, level: usize) -> EntryKind {
        if raw & VALID_BIT == 0 {
            return EntryKind::Invalid;
        }

        match level {
            // L0: TYPE=1 means table, TYPE=0 is reserved/invalid
            0 => {
                if raw & TYPE_BIT != 0 {
                    EntryKind::Table(PhysAddr::new(raw & TABLE_ADDR_MASK))
                } else {
                    EntryKind::Invalid
                }
            }
            // L1: TYPE=1 means table, TYPE=0 means 512MiB block
            1 => {
                if raw & TYPE_BIT != 0 {
                    EntryKind::Table(PhysAddr::new(raw & TABLE_ADDR_MASK))
                } else {
                    EntryKind::Block(PhysAddr::new(raw & L1_BLOCK_ADDR_MASK))
                }
            }
            // L2: TYPE=1 means valid page, TYPE=0 is reserved/invalid
            2 => {
                if raw & TYPE_BIT != 0 {
                    EntryKind::Block(PhysAddr::new(raw & L2_PAGE_ADDR_MASK))
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
            1 => L1_BLOCK_ADDR_MASK,
            _ => panic!("Block entries only valid at L1 for 64K granule"),
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
            addr & !L2_PAGE_ADDR_MASK == 0,
            "Page address not 64K aligned"
        );
        (addr & L2_PAGE_ADDR_MASK) | encode_attributes(attr) | AF_BIT | TYPE_BIT | VALID_BIT
    }

    fn output_address(raw: u64, level: usize) -> PhysAddr {
        let mask = match level {
            0 => TABLE_ADDR_MASK,
            1 => {
                if raw & TYPE_BIT != 0 {
                    TABLE_ADDR_MASK
                } else {
                    L1_BLOCK_ADDR_MASK
                }
            }
            2 => L2_PAGE_ADDR_MASK,
            _ => 0,
        };
        PhysAddr::new(raw & mask)
    }
}
