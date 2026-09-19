use {crate::objects::NucleusObject, libobject::ObjectType};

pub struct AArch64ASID;

impl NucleusObject for AArch64ASID {
    const TYPE: ObjectType = ObjectType::ASID;
    const POOL: crate::objects::access::PoolTag = crate::objects::access::PoolTag::ASID;
}
