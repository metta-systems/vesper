use {crate::objects::NucleusObject, libaddress::PhysAddr, libobject::ObjectType};

pub struct AArch64PageTable;

impl AArch64PageTable {
    pub fn new(_addr: PhysAddr) -> Self {
        Self
    }
}

impl NucleusObject for AArch64PageTable {
    const TYPE: ObjectType = ObjectType::PAGE_TABLE;
    const POOL: crate::objects::access::PoolTag = crate::objects::access::PoolTag::PageTable;
}
