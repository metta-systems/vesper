use {crate::objects::NucleusObject, libobject::ObjectType};

pub struct AArch64ASID;

impl NucleusObject for AArch64ASID {
    const TYPE: ObjectType = ObjectType::ASID;
}
