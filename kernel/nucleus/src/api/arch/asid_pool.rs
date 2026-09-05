pub fn invoke<A: ArchObjects>(
    pool: &mut A::ASIDPool,
    rights: Rights,
    op: u32,
    args: &[u64; 6],
    kernel: &mut Kernel<A>,
) -> Result<(u64, u64), CapError> {
    todo!("asid_pool invoke")
}
