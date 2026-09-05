use {crate::objects::NucleusObject, libobject::ObjectType};

pub struct AArch64VSpace;

impl AArch64VSpace {
    pub fn new() -> Self {
        Self
    }
}

impl NucleusObject for AArch64VSpace {
    const TYPE: ObjectType = ObjectType::VSPACE;
}
