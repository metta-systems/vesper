//! Translation table.

#[cfg(target_arch = "aarch64")]
use crate::arch::aarch64::memory::mmu::translation_table as arch_translation_table;

//--------------------------------------------------------------------------------------------------
// Architectural Public Reexports
//--------------------------------------------------------------------------------------------------
pub use arch_translation_table::KernelTranslationTable;
