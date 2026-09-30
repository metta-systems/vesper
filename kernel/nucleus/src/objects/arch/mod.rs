#[cfg(target_arch = "aarch64")]
pub mod aarch64_objects;
#[cfg(target_arch = "aarch64")]
pub use aarch64_objects::AArch64 as ArchObjectsImpl;

pub mod arch_pools;
pub use arch_pools::ArchPools;

#[cfg(target_arch = "aarch64")]
pub mod address_space;
#[cfg(target_arch = "aarch64")]
pub use address_space::AArch64AddressSpace;

#[cfg(target_arch = "aarch64")]
pub mod asid_control;
#[cfg(target_arch = "aarch64")]
pub use asid_control::AArch64ASIDControl;

#[cfg(target_arch = "aarch64")]
pub mod asid_pool;
#[cfg(target_arch = "aarch64")]
pub use asid_pool::AArch64ASIDPool;

#[cfg(target_arch = "aarch64")]
pub mod frame;

#[cfg(target_arch = "aarch64")]
pub mod page_table;
#[cfg(target_arch = "aarch64")]
pub use page_table::AArch64PageTable;
