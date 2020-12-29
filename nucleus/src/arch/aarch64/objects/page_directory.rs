/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

// probably just impl some Mapping trait for these "structs"?

// L2 table
struct PageDirectory {}

impl PageDirectory {
    fn map(
        pud: PageUpperDirectory, /*Cap*/
        vaddr: VirtAddr,
        attr: VMAttributes,
    ) -> Result<()> {
        todo!()
    }
    fn unmap() -> Result<()> {
        todo!()
    }
}
