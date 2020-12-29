/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

use {
    crate::{
        capdef,
        caps::{CapError, Capability},
    },
    core::convert::TryFrom,
    paste::paste,
    register::{register_bitfields, LocalRegisterCopy},
};

//=====================
// Cap definition
//=====================

register_bitfields! {
    u128,
    PageDirectoryCap [
        Type OFFSET(0) NUMBITS(6) [
            value = 5
        ],
        IsMapped OFFSET(6) NUMBITS(1) [],
        BasePtr OFFSET(16) NUMBITS(48) [], // PhysAddr
        MappedAddress OFFSET(64) NUMBITS(48) [], // VirtAddr
        MappedASID OFFSET(112) NUMBITS(16) [],
    ]
}

capdef! { PageDirectory }

//=====================
// Cap implementation
//=====================

impl PageDirectoryCapability {
    pub(crate) fn base_address() -> PhysAddr {
        PhysAddr::new(self.0.read(PageDirectoryCap::BasePtr))
    }

    pub(crate) fn is_mapped() -> bool {
        self.0.read(PageDirectoryCap::IsMapped) == 1
    }

    pub(crate) fn mapped_address() -> VirtAddr {
        VirtAddr::new(self.0.read(PageDirectoryCap::MappedAddress))
    }

    pub(crate) fn mapped_asid() -> ASID {
        self.0.read(PageDirectoryCap::MappedASID)
    }
}
