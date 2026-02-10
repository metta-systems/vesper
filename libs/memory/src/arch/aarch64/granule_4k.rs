use {
    super::common::*,
    crate::arch_trait::{EntryKind, LevelCapabilities, TranslationArch},
    libaddress::{PhysAddr, VirtAddr},
    libmapping::AttributeFields,
};

// ---------------------------------------------------------------------------
// AArch64 Stage 1, 4KiB granule
// ---------------------------------------------------------------------------
//
//  Virtual address split (48-bit VA):
//
//    63-48    47-39    38-30    29-21    20-12    11-0
//    signx     L0       L1       L2       L3      off
//
//  Each level indexes 512 entries (9 bits). Table size = 512 * 8 = 4096 bytes.
//
//  L0: table pointer only
//  L1: table pointer or 1GiB block
//  L2: table pointer or 2MiB block
//  L3: 4KiB page only (TYPE bit 1 = 1 for valid page)

// Address masks (4K granule specific)
const L1_BLOCK_ADDR_MASK: u64 = 0x0000_FFFF_C000_0000; // [47:30]
const L2_BLOCK_ADDR_MASK: u64 = 0x0000_FFFF_FFE0_0000; // [47:21]
const L3_PAGE_ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000; // [47:12]

// Block sizes
const SIZE_4K: usize = 4096;
const SIZE_2M: usize = 2 * 1024 * 1024;
const SIZE_1G: usize = 1024 * 1024 * 1024;

/// AArch64 Stage 1 translation with 4KiB granule.
///
/// 4-level hierarchy: L0 -> L1 -> L2 -> L3.
/// 512 entries per table, 4096-byte table size, 4096-byte alignment.
pub struct Aarch64_4K;

impl TranslationArch for Aarch64_4K {
    const NUM_LEVELS: usize = 4;

    fn entries_per_table(_level: usize) -> usize {
        512
    }

    fn table_alignment(_level: usize) -> usize {
        SIZE_4K
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
                block_size: SIZE_1G,
            },
            2 => LevelCapabilities {
                supports_table_pointer: true,
                supports_block: true,
                block_size: SIZE_2M,
            },
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
            0 => 39,
            1 => 30,
            2 => 21,
            3 => 12,
            _ => return 0,
        };
        ((vaddr.as_u64() >> shift) & 0x1FF) as usize
    }

    fn decode_entry(raw: u64, level: usize) -> EntryKind {
        if raw & VALID_BIT == 0 {
            return EntryKind::Invalid;
        }

        match level {
            0 => {
                if raw & TYPE_BIT != 0 {
                    EntryKind::Table(PhysAddr::new(raw & TABLE_ADDR_MASK))
                } else {
                    EntryKind::Invalid
                }
            }
            1 => {
                if raw & TYPE_BIT != 0 {
                    EntryKind::Table(PhysAddr::new(raw & TABLE_ADDR_MASK))
                } else {
                    EntryKind::Block(PhysAddr::new(raw & L1_BLOCK_ADDR_MASK))
                }
            }
            2 => {
                if raw & TYPE_BIT != 0 {
                    EntryKind::Table(PhysAddr::new(raw & TABLE_ADDR_MASK))
                } else {
                    EntryKind::Block(PhysAddr::new(raw & L2_BLOCK_ADDR_MASK))
                }
            }
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
        debug_assert!(addr & !TABLE_ADDR_MASK == 0, "Table address not 4K aligned");
        (addr & TABLE_ADDR_MASK) | TYPE_BIT | VALID_BIT
    }

    fn encode_block_entry(phys: PhysAddr, attr: AttributeFields, level: usize) -> u64 {
        let addr = phys.as_u64();
        let addr_mask = match level {
            1 => L1_BLOCK_ADDR_MASK,
            2 => L2_BLOCK_ADDR_MASK,
            _ => panic!("Block entries only valid at L1 and L2"),
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
            "Page address not 4K aligned"
        );
        (addr & L3_PAGE_ADDR_MASK) | encode_attributes(attr) | AF_BIT | TYPE_BIT | VALID_BIT
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
