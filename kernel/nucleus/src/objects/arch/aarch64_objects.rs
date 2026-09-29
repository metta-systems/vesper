use {
    crate::objects::{
        ArchObjects,
        access::ObjectId,
        arch::{
            AArch64ASIDControl, AArch64ASIDPool, AArch64AddressSpace, AArch64PageTable, ArchPools,
        },
        arch_objects::FrameSize,
        key_table::KeyTableBinding,
    },
    libaddress::PhysAddr,
    libobject::{ArchType, CapError, ObjectType},
};

// ═══════════════════════════════════════════════════════════════════
// AARCH64 IMPLEMENTATION
// ═══════════════════════════════════════════════════════════════════

pub struct AArch64;

/// Encode the ASID and VA page number for `TLBI VAE1IS`.
/// Low VA offset bits are discarded; operand bits 43:0 contain VA[55:12].
pub const fn tlbi_vae1is_operand(asid: u16, vaddr: u64) -> u64 {
    ((asid as u64) << 48) | ((vaddr >> 12) & 0x0000_0FFF_FFFF_FFFF)
}

/// Validate the existing 4 KiB, non-LPA2 TTBR0 encoding against the active
/// physical-address/ASID widths. Kept pure so rejected metadata needs no MMU
/// transition in tests. Zero is a valid physical table address, not a sentinel.
fn validate_translation_metadata(
    root: u64,
    asid: u16,
    tcr: u64,
    mmfr0: u64,
) -> Result<(), CapError> {
    let physical_bits = match (tcr >> 32) & 7 {
        0 => 32,
        1 => 36,
        2 => 40,
        3 => 42,
        4 => 44,
        5 => 48,
        // The current descriptor/TTBR encoding does not support 52-bit PA.
        _ => return Err(CapError::InvalidOperation),
    };
    let hardware_physical_bits = match mmfr0 & 0xF {
        0 => 32,
        1 => 36,
        2 => 40,
        3 => 42,
        4 => 44,
        5 => 48,
        6 => 52,
        _ => return Err(CapError::InvalidOperation),
    };
    if physical_bits > hardware_physical_bits {
        return Err(CapError::InvalidOperation);
    }
    if root & 0xFFF != 0 || root >> physical_bits != 0 {
        return Err(CapError::InvalidPointer);
    }
    // Tables are L0-rooted with a 48-bit VA range, and TTBR0 walks must be
    // enabled. A1 must select TTBR0's ASID. DS/LPA2 and other granules would
    // need a different root encoding; reject rather than truncate/alias.
    if tcr & 0x3F != 16 || tcr & ((1 << 7) | (1 << 22) | (1 << 59) | (3 << 14)) != 0 {
        return Err(CapError::InvalidOperation);
    }
    let wide_asid = tcr & (1 << 36) != 0;
    let hardware_wide = (mmfr0 >> 4) & 0xF == 2;
    if asid == 0 || (wide_asid && !hardware_wide) || (!wide_asid && asid > 255) {
        return Err(CapError::InvalidOperation);
    }
    Ok(())
}

impl ArchObjects for AArch64 {
    type PageTable = AArch64PageTable;
    type AddressSpace = AArch64AddressSpace;
    type ASIDPool = AArch64ASIDPool;
    type ASIDControl = AArch64ASIDControl;

    const FRAME_SIZES: &'static [FrameSize] =
        &[FrameSize::Small, FrameSize::Large, FrameSize::Huge];

    const PT_LEVELS: usize = 4;
    const PT_INDEX_BITS: usize = 9;

    fn validate_frame_size(size_bits: u8) -> Result<usize, CapError> {
        match size_bits {
            12 => Ok(4096),               // 4KB
            21 => Ok(2 * 1024 * 1024),    // 2MB
            30 => Ok(1024 * 1024 * 1024), // 1GB
            _ => Err(CapError::InvalidFrameSize(size_bits as usize)),
        }
    }

    fn validate_retype(arch_type: ArchType, size_bits: u8) -> Result<usize, CapError> {
        match arch_type {
            ArchType::PageTable => {
                if size_bits == 12 {
                    Ok(4096)
                } else {
                    Err(CapError::InvalidSize(size_bits as usize))
                }
            }
            ArchType::AddressSpace => Ok(core::mem::size_of::<AArch64AddressSpace>()),
            ArchType::ASIDPool => Ok(core::mem::size_of::<AArch64ASIDPool>()),
            ArchType::ASIDControl => Ok(core::mem::size_of::<AArch64ASIDControl>()),
            _ => Err(CapError::UnsupportedArchType(arch_type)),
        }
    }

    fn new_page_table(paddr: u64) -> AArch64PageTable {
        AArch64PageTable::new(paddr)
    }

    fn new_asid_pool() -> AArch64ASIDPool {
        AArch64ASIDPool::new()
    }

    fn new_address_space(keytable: KeyTableBinding) -> AArch64AddressSpace {
        AArch64AddressSpace::new(keytable)
    }

    fn invalidate_tlb_by_vaddr(asid: u16, vaddr: u64) {
        // Operand format of `TLBI VAE1IS`: ASID in bits 63:48 and
        // VA[55:12] in bits 43:0, not a page-aligned byte address.
        let value = tlbi_vae1is_operand(asid, vaddr);
        // SAFETY: TLB maintenance is a hardware side effect; the instructions
        // read no memory and clobber no registers. The barrier sequence
        // publishes descriptor stores before invalidation and guarantees it
        // completes before any subsequent access could observe the withdrawn
        // translation.
        unsafe {
            core::arch::asm!(
                "dsb ishst",
                "tlbi vae1is, {value}",
                "dsb sy",
                "isb",
                value = in(reg) value,
                options(nostack),
            );
        }
    }

    fn invalidate_tlb_asid(asid: u16) {
        // Operand format of `TLBI ASIDE1IS`: ASID in bits 63:48.
        let value = u64::from(asid) << 48;
        // SAFETY: see `invalidate_tlb_by_vaddr`.
        unsafe {
            core::arch::asm!(
                "dsb ishst",
                "tlbi aside1is, {value}",
                "dsb sy",
                "isb",
                value = in(reg) value,
                options(nostack),
            );
        }
    }

    fn validate_translation_context(root_paddr: u64, asid: u16) -> Result<(), CapError> {
        use aarch64_cpu::registers::{ID_AA64MMFR0_EL1, Readable, TCR_EL1};

        validate_translation_metadata(root_paddr, asid, TCR_EL1.get(), ID_AA64MMFR0_EL1.get())
    }

    fn install_translation_context(root_paddr: u64, asid: u16) {
        // TTBR0_EL1: ASID in bits 63:48, translation-table base in BADDR.
        // The kernel executes through the TTBR1 high map, so switching the
        // low-half context does not disturb kernel execution.
        let ttbr = root_paddr | (u64::from(asid) << 48);
        // SAFETY: register write plus context synchronization; reads no
        // memory and clobbers no registers. `dsb ish` publishes the prior
        // descriptor stores to page-table walks before the context switch;
        // `isb` orders the new context before any subsequent translation.
        unsafe {
            core::arch::asm!(
                "dsb ish",
                "msr ttbr0_el1, {ttbr}",
                "isb",
                ttbr = in(reg) ttbr,
                options(nostack),
            );
        }
    }

    fn install_table_entry(
        parent_paddr: u64,
        parent_level: u8,
        vaddr: u64,
        child_paddr: u64,
    ) -> Result<u16, CapError> {
        super::page_table::install_table_entry(parent_paddr, parent_level, vaddr, child_paddr)
    }

    fn clear_table_entry(parent_paddr: u64, slot: u16, child_paddr: u64) -> Result<(), CapError> {
        super::page_table::clear_table_entry(parent_paddr, slot, child_paddr)
    }

    fn page_table_is_empty(paddr: u64) -> bool {
        super::page_table::table_is_empty(paddr)
    }

    fn install_frame_pte(
        root_paddr: u64,
        vaddr: u64,
        frame_paddr: u64,
        size_bits: u8,
        writable: bool,
        executable: bool,
    ) -> Result<(), CapError> {
        super::page_table::install_frame_pte(
            root_paddr,
            vaddr,
            frame_paddr,
            size_bits,
            writable,
            executable,
        )
    }

    fn clear_frame_pte(
        root_paddr: u64,
        vaddr: u64,
        frame_paddr: u64,
        size_bits: u8,
    ) -> Result<(), CapError> {
        super::page_table::clear_frame_pte(root_paddr, vaddr, frame_paddr, size_bits)
    }

    fn find_physical_overlap(root_paddr: u64, paddr: u64, size_bits: u8) -> Option<u64> {
        super::page_table::find_physical_overlap(root_paddr, paddr, size_bits)
    }

    fn create_arch_object(
        arch_type: ArchType,
        phys_addr: PhysAddr,
        size_bits: u8,
        pools: &mut ArchPools<Self>,
    ) -> Result<(ObjectType, ObjectId), CapError> {
        // Implementation status: AddressSpace provisioning requires an
        // initialized KeyTableBinding; this excluded creation sketch has no
        // selected public provisioning schema yet.
        // match arch_type {
        //     ArchType::Frame => {
        //         let frame_size = FrameSize::from_bits(size_bits as usize)
        //             .map_err(|_| CapError::InvalidSize(size_bits as usize))?;
        //         let frame = AArch64Frame::new(phys_addr, frame_size);
        //         let (id, _obj) = pools
        //             .frames
        //             .allocate(frame)
        //             .ok_or(CapError::PoolExhausted)?;
        //         Ok((ObjectType::FRAME, id))
        //     }
        //     ArchType::PageTable => {
        //         let pt = AArch64PageTable::new(phys_addr);
        //         let (id, _obj) = pools
        //             .page_tables
        //             .allocate(pt)
        //             .ok_or(CapError::PoolExhausted)?;
        //         Ok((ObjectType::PAGE_TABLE, id))
        //     }
        //     ArchType::AddressSpace => {
        //         let address_space = AArch64AddressSpace::new();
        //         let (id, _obj) = pools
        //             .address_spaces
        //             .allocate(address_space)
        //             .ok_or(CapError::PoolExhausted)?;
        //         Ok((ObjectType::ADDRESS_SPACE, id))
        //     }
        //     ArchType::ASIDPool => {
        //         let pool = AArch64ASIDPool::new();
        //         let (id, _obj) = pools
        //             .asid_pools
        //             .allocate(pool)
        //             .ok_or(CapError::PoolExhausted)?;
        //         Ok((ObjectType::ASID_POOL, id))
        //     }
        //     _ => Err(CapError::UnsupportedArchType(arch_type)),
        // }
        Err(CapError::UnsupportedArchType(arch_type))
    }

    // ─────────────────────────────────────────────────────────────────
    // Frame, PageTable, AddressSpace, and ASIDPool operations are dispatched
    // directly to their API handlers; see
    // `crate::api::arch::{frame,page_table,address_space,asid_pool}`.
    // ─────────────────────────────────────────────────────────────────
}

#[cfg(test)]
mod tests {
    use {
        super::{tlbi_vae1is_operand, validate_translation_metadata},
        libobject::CapError,
    };

    #[test_case]
    fn translation_metadata_rejects_root_truncation_and_alignment() {
        assert!(validate_translation_metadata(0, 1, 16, 0).is_ok());
        assert!(validate_translation_metadata(0xFFFF_F000, 255, 16, 0).is_ok());
        for root in [1, 0x1001, 1 << 32, 1 << 48, 1 << 52, u64::MAX] {
            assert!(matches!(
                validate_translation_metadata(root, 1, 16, 0),
                Err(CapError::InvalidPointer)
            ));
        }
        assert!(validate_translation_metadata(0x0000_FFFF_FFFF_F000, 1, 16 | (5 << 32), 5).is_ok());
        assert!(matches!(
            validate_translation_metadata(1 << 48, 1, 16 | (5 << 32), 5),
            Err(CapError::InvalidPointer)
        ));
        assert!(matches!(
            validate_translation_metadata(0x1000, 1, 16 | (6 << 32), 0),
            Err(CapError::InvalidOperation)
        ));
    }

    #[test_case]
    fn translation_metadata_rejects_asid_aliases_and_unsupported_encoding() {
        for (asid, tcr, mmfr0) in [
            (0, 0, 0),
            (256, 0, 0),
            (65535, 0, 2 << 4),
            (256, 1 << 36, 0),
            (1, 1 << 22, 0),
            (1, 1 << 59, 0),
            (1, 1 << 14, 0),
            (1, 2 << 14, 0),
            (1, 1 << 32, 0), // IPS exceeds hardware PARange.
            (1, 0, 0xF),     // Reserved hardware PARange encoding.
        ] {
            assert!(matches!(
                validate_translation_metadata(0x1000, asid, 16 | tcr, mmfr0),
                Err(CapError::InvalidOperation)
            ));
        }
        assert!(validate_translation_metadata(0x1000, 65535, 16 | (1 << 36), 2 << 4).is_ok());
    }

    #[test_case]
    fn translation_metadata_requires_enabled_l0_rooted_ttbr0_walks() {
        for t0sz in 0..=63 {
            if t0sz == 16 {
                continue;
            }
            assert!(matches!(
                validate_translation_metadata(0x1000, 1, t0sz, 0),
                Err(CapError::InvalidOperation)
            ));
        }
        assert!(matches!(
            validate_translation_metadata(0x1000, 1, 16 | (1 << 7), 0),
            Err(CapError::InvalidOperation)
        ));
        assert!(validate_translation_metadata(0x1000, 1, 16, 0).is_ok());
    }

    #[test_case]
    fn vae1is_encodes_the_va_page_number_not_a_byte_address() {
        const OPERAND: u64 = tlbi_vae1is_operand(1, 0x1000_0000);
        assert_eq!(OPERAND, 0x0001_0000_0001_0000);
        assert_eq!(tlbi_vae1is_operand(1, 0x1000_0FFF), OPERAND);
        assert_eq!(tlbi_vae1is_operand(1, 0x1000_1000), OPERAND + 1);
        assert_eq!(
            tlbi_vae1is_operand(0x1234, 0x0000_1234_5678_9ABC),
            0x1234_0001_2345_6789
        );
    }

    #[test_case]
    fn vae1is_keeps_asid_and_va_fields_separate() {
        assert_eq!(tlbi_vae1is_operand(0, 0), 0);
        assert_eq!(tlbi_vae1is_operand(0xFFFF, 0), 0xFFFF_0000_0000_0000);
        assert_eq!(tlbi_vae1is_operand(0xFFFF, u64::MAX), 0xFFFF_0FFF_FFFF_FFFF);
        assert_eq!(tlbi_vae1is_operand(0, 0xFF00_0000_0000_0000), 0);
        assert_eq!(
            tlbi_vae1is_operand(0x1234, 0xFFFF_8000_1234_5678),
            0x1234_0FF8_0001_2345
        );
    }
}
