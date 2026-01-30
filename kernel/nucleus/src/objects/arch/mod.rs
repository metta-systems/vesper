#[cfg(target_arch = "aarch64")]
pub mod aarch64_objects;
#[cfg(target_arch = "aarch64")]
pub use aarch64_objects::AArch64 as ArchObjectsImpl;
