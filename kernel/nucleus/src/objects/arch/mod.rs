#[cfg(target_arch = "aarch64")]
pub mod aarch64_objects;
#[cfg(target_arch = "aarch64")]
pub use aarch64_objects::AArch64 as ArchObjectsImpl;

#[cfg(target_arch = "aarch64")]
pub mod frame;
#[cfg(target_arch = "aarch64")]
pub use frame::AArch64Frame;
