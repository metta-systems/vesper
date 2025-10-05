pub type KernelGranule = crate::mmu::TranslationGranule<{ 64 * 1024 }>;

pub mod raspberrypi;
pub use raspberrypi::*;
