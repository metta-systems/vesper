/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

// L3 tables
struct PageTable {}

impl PageTable {
    fn map(virt_space: VirtSpace /*Cap*/, vaddr: VirtAddr, attr: VMAttributes) -> Result<()> {
        todo!()
    }
    fn unmap() -> Result<()> {
        todo!()
    }
}
