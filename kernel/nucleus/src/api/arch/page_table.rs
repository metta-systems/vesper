pub struct AArch64PageTable;

#[repr(u8)]
pub enum PageTableOp {
    /// Map this page table into parent table or VSpace
    Map = 0,
    /// Unmap from parent
    Unmap = 1,
}

pub fn invoke<A: ArchObjects>(
    pt: &mut A::PageTable,
    rights: Rights,
    op: u32,
    args: &[u64; 6],
    kernel: &mut Kernel<A>,
) -> Result<(u64, u64), CapError> {
    match op {
        PageTableOp::Map => {
            // args[0] = vspace_slot
            // args[1] = virt_addr (determines which slot in parent)
            let vspace_slot = KeySlot(args[0] as u16);
            let virt_addr = VirtAddr::new(args[1]);

            // ... mapping logic
            todo!("page_table map")
        }

        PageTableOp::Unmap => {
            todo!("page_table unmap")
        }
    }
}
