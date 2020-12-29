/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

// L1 table
struct PageUpperDirectory {}

impl PageUpperDirectory {
    fn map(
        pgd: PageGlobalDirectory, /*Cap*/
        vaddr: VirtAddr,
        attr: VMAttributes,
    ) -> Result<()> {
        todo!()
    }
    fn unmap() -> Result<()> {
        todo!()
    }
}
