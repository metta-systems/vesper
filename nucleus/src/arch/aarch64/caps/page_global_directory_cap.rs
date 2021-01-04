/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

use {
    crate::{
        arch::memory::{PhysAddr, VirtAddr, ASID},
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
    PageGlobalDirectoryCap [
        Type OFFSET(0) NUMBITS(6) [
            value = 9
        ],
        IsMapped OFFSET(6) NUMBITS(1) [],
        BasePtr OFFSET(16) NUMBITS(48) [], // PhysAddr
        MappedASID OFFSET(112) NUMBITS(16) [],
    ]
}

capdef! { PageGlobalDirectory }

//=====================
// Cap implementation
//=====================

impl PageGlobalDirectoryCapability {
    pub(crate) fn base_address(&self) -> PhysAddr {
        PhysAddr::new(self.0.read(PageGlobalDirectoryCap::BasePtr))
    }

    pub(crate) fn is_mapped(&self) -> bool {
        self.0.read(PageGlobalDirectoryCap::IsMapped) == 1
    }

    // Global directory does not give access to mapped addresses,
    // instead, it links to lower page directory levels.

    pub(crate) fn mapped_asid(&self) -> ASID {
        self.0.read(PageGlobalDirectoryCap::MappedASID)
    }
}
