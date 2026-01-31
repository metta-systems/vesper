// ═══════════════════════════════════════════════════════════════════
// ARCH OBJECTS TRAIT WITH INVOKE METHODS
// ═══════════════════════════════════════════════════════════════════

use libsyscall::CapError;

use crate::objects::NucleusObject;

/// Architecture abstraction trait - extended with invoke methods
pub trait ArchObjects: Sized + 'static {
    // ─── Associated Types ───
    type Frame: NucleusObject;
    type PageTable: NucleusObject;
    type VSpace: NucleusObject;
    type ASIDPool: NucleusObject;
    type ASID: NucleusObject;

    // ─── Constants ───
    const FRAME_SIZES: &'static [FrameSize];
    const PT_LEVELS: usize;
    const PT_INDEX_BITS: usize;

    // ─── Validation ───
    fn validate_retype(arch_type: ArchType, size_bits: u8) -> Result<usize, CapError>;

    // ─── Object Creation ───
    fn create_arch_object(
        arch_type: ArchType,
        phys_addr: PhysAddr,
        size_bits: u8,
        pools: &mut ArchPools<Self>,
    ) -> Result<ObjectRef, CapError>;

    // ─── Invocation Handlers ───
    fn invoke_frame(
        frame: &mut Self::Frame,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
        kernel: &mut Kernel<Self>,
    ) -> Result<(u64, u64), CapError>;

    fn invoke_page_table(
        pt: &mut Self::PageTable,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
        kernel: &mut Kernel<Self>,
    ) -> Result<(u64, u64), CapError>;

    fn invoke_vspace(
        vspace: &mut Self::VSpace,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
        kernel: &mut Kernel<Self>,
    ) -> Result<(u64, u64), CapError>;

    fn invoke_asid_pool(
        pool: &mut Self::ASIDPool,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
        kernel: &mut Kernel<Self>,
    ) -> Result<(u64, u64), CapError>;

    fn invoke_asid(
        asid: &mut Self::ASID,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
    ) -> Result<(u64, u64), CapError>;

    // Optional - default implementations return UnsupportedArchType
    fn invoke_io_space(
        _entry: &mut KeyEntry,
        _op: u32,
        _args: &[u64; 6],
        _kernel: &mut Kernel<Self>,
    ) -> Result<(u64, u64), CapError> {
        Err(CapError::UnsupportedArchType(ArchType::IOSpace))
    }

    fn invoke_irq_handler(
        _entry: &mut KeyEntry,
        _op: u32,
        _args: &[u64; 6],
        _kernel: &mut Kernel<Self>,
    ) -> Result<(u64, u64), CapError> {
        Err(CapError::UnsupportedArchType(ArchType::IRQHandler))
    }

    fn invoke_irq_control(
        _entry: &mut KeyEntry,
        _op: u32,
        _args: &[u64; 6],
        _kernel: &mut Kernel<Self>,
    ) -> Result<(u64, u64), CapError> {
        Err(CapError::UnsupportedArchType(ArchType::IRQControl))
    }
}
