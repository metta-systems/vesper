/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */
// Allocate regions from boot memory list obtained from devtree
pub struct BootRegionAllocator {}

impl BootRegionAllocator {
    pub fn new(&boot_info: BootInfo) -> Self {
        Self {}
    }

    pub fn alloc_region(&mut self) {}

    pub fn alloc_zeroed(&mut self) {}
}
