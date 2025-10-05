//! Translation table.

#[cfg(target_arch = "aarch64")]
use crate::arch::aarch64::mmu::translation_table as arch_translation_table;

use {
    super::{AttributeFields, MemoryRegion},
    crate::{Address, Physical, Virtual},
};

//--------------------------------------------------------------------------------------------------
// Architectural Public Reexports
//--------------------------------------------------------------------------------------------------
#[cfg(target_arch = "aarch64")]
pub use arch_translation_table::FixedSizeTranslationTable;

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Translation table interfaces.
pub mod interface {
    use super::*;

    /// Translation table operations.
    pub trait TranslationTable {
        /// Anything that needs to run before any of the other provided functions can be used.
        ///
        /// # Safety
        ///
        /// - Implementor must ensure that this function can run only once or is harmless if invoked
        ///   multiple times.
        fn init(&mut self);

        /// The translation table's base address to be used for programming the MMU.
        fn phys_base_address(&self) -> Address<Physical>;

        /// Map the given virtual memory region to the given physical memory region.
        ///
        /// # Safety
        ///
        /// - Using wrong attributes can cause multiple issues of different nature in the system.
        /// - It is not required that the architectural implementation prevents aliasing. That is,
        ///   mapping to the same physical memory using multiple virtual addresses, which would
        ///   break Rust's ownership assumptions. This should be protected against in the kernel's
        ///   generic MMU code.
        unsafe fn map_at(
            &mut self,
            virt_region: &MemoryRegion<Virtual>,
            phys_region: &MemoryRegion<Physical>,
            attr: &AttributeFields,
        ) -> Result<(), &'static str>;
    }
}
