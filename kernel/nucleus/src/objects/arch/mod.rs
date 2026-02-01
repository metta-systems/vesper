#[cfg(target_arch = "aarch64")]
pub mod aarch64_objects;
#[cfg(target_arch = "aarch64")]
pub use aarch64_objects::AArch64 as ArchObjectsImpl;

pub mod arch_pools;
pub use arch_pools::ArchPools;

#[cfg(target_arch = "aarch64")]
pub mod asid;
#[cfg(target_arch = "aarch64")]
pub use asid::AArch64ASID;

#[cfg(target_arch = "aarch64")]
pub mod asid_pool;
#[cfg(target_arch = "aarch64")]
pub use asid_pool::AArch64ASIDPool;

#[cfg(target_arch = "aarch64")]
pub mod frame;
#[cfg(target_arch = "aarch64")]
pub use frame::AArch64Frame;

#[cfg(target_arch = "aarch64")]
pub mod page_table;
#[cfg(target_arch = "aarch64")]
pub use page_table::AArch64PageTable;

#[cfg(target_arch = "aarch64")]
pub mod vspace;
#[cfg(target_arch = "aarch64")]
pub use vspace::AArch64VSpace;
