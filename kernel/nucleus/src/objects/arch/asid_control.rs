use {crate::objects::NucleusObject, libobject::ObjectType};

/// Placeholder for the reserved `ASIDControl` arch kind: the capability-protected
/// control point over the ASID namespace — the expected home for pool
/// creation/partitioning (seL4-style).
///
/// An ASID is a hardware naming value bound to an `AddressSpace`, not an
/// independently capabilitied object, so there is no per-ASID capability
/// kind. Its operation schema is open; the kind stays
/// reserved/unsupported (no pool, no handler) until that contract is
/// specified.
pub struct AArch64ASIDControl;

impl NucleusObject for AArch64ASIDControl {
    const TYPE: ObjectType = ObjectType::ASID_CONTROL;
    const POOL: crate::objects::access::PoolTag = crate::objects::access::PoolTag::ASIDControl;
}
