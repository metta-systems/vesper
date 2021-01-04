/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

use crate::memory::{mmu::PageGlobalDirectory, VirtAddr};

// L1 table
struct PageUpperDirectory {}

impl PageUpperDirectory {
    fn map(
        pgd: PageGlobalDirectory, /*Cap*/
        vaddr: VirtAddr,
        attr: u32, //VMAttributes,
    ) -> Result<()> {
        todo!()
    }
    fn unmap() -> Result<()> {
        todo!()
    }
}
