pub fn invoke<A: ArchObjects>(
    asid: &mut A::ASID,
    rights: Rights,
    op: u32,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    // ASIDs mostly just exist; operations are minimal
    todo!("asid invoke")
}
