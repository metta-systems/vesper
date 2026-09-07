use {crate::objects::NucleusObject, libobject::ObjectType};

pub struct AArch64ASIDPool;

impl AArch64ASIDPool {
    pub fn new() -> Self {
        Self
    }
}

impl NucleusObject for AArch64ASIDPool {
    const TYPE: ObjectType = ObjectType::ASID_POOL;
    const POOL: crate::objects::access::PoolTag = crate::objects::access::PoolTag::ASIDPool;
}
