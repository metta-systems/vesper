/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

struct Page {}

impl Page {
    // VirtSpace-like interface.
    /// Get the physical address of the underlying frame.
    fn get_address() -> Result<PhysAddr> {
        todo!()
    }
    fn map(
        virt_space: VirtSpace, /*Cap*/
        vaddr: VirtAddr,
        rights: CapRights,
        attr: VMAttributes,
    ) -> Result<()> {
        todo!()
    }
    /// Changes the permissions of an existing mapping.
    fn remap(
        virt_space: VirtSpace, /*Cap*/
        rights: CapRights,
        attr: VMAttributes,
    ) -> Result<()> {
        todo!()
    }
    fn unmap() -> Result<()> {
        todo!()
    }
    // MMIO space.
    fn map_io(iospace: IoSpace /*Cap*/, rights: CapRights, ioaddr: VirtAddr) -> Result<()> {
        todo!()
    }
}

impl PageCacheManagement for Page {
    fn clean_data(start_offset: usize, end_offset: usize) -> _ {
        todo!()
    }

    fn clean_invalidate_data(start_offset: usize, end_offset: usize) -> _ {
        todo!()
    }

    fn invalidate_data(start_offset: usize, end_offset: usize) -> _ {
        todo!()
    }

    fn unify_instruction_cache(start_offset: usize, end_offset: usize) -> _ {
        todo!()
    }
}
