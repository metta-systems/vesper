use {
    crate::println,
    core::{
        fmt::{self, Formatter},
        ops::RangeInclusive,
    },
    snafu::Snafu,
};

#[cfg(target_arch = "aarch64")]
use crate::arch::aarch64::memory::mmu as arch_mmu;

pub mod translation_table;

//--------------------------------------------------------------------------------------------------
// Architectural Public Reexports
//--------------------------------------------------------------------------------------------------
pub use arch_mmu::mmu;

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// MMU enable errors variants.
#[allow(missing_docs)]
#[derive(Debug, Snafu)]
pub enum MMUEnableError {
    #[snafu(display("MMU is already enabled"))]
    AlreadyEnabled,
    #[snafu(display("{}", err))]
    Other { err: &'static str },
}

/// Memory Management interfaces.
pub mod interface {
    use super::*;

    /// MMU functions.
    pub trait MMU {
        /// Called by the kernel during early init. Supposed to take the translation tables from the
        /// `BSP`-supplied `virt_mem_layout()` and install/activate them for the respective MMU.
        ///
        /// # Safety
        ///
        /// - Changes the HW's global state.
        unsafe fn enable_mmu_and_caching(&self) -> Result<(), MMUEnableError>;

        /// Returns true if the MMU is enabled, false otherwise.
        fn is_enabled(&self) -> bool;

        fn print_features(&self); // debug
    }
}

/// Describes the characteristics of a translation granule.
pub struct TranslationGranule<const GRANULE_SIZE: usize>;

/// Describes properties of an address space.
pub struct AddressSpace<const AS_SIZE: usize>;

/// Architecture agnostic memory attributes.
#[derive(Copy, Clone)]
pub enum MemAttributes {
    /// Regular memory
    CacheableDRAM,
    /// Memory without caching
    NonCacheableDRAM,
    /// Device memory
    Device,
}

/// Architecture agnostic memory region access permissions.
#[derive(Copy, Clone)]
pub enum AccessPermissions {
    /// Read-only access
    ReadOnly,
    /// Read-write access
    ReadWrite,
}

// Architecture agnostic memory region translation types.
#[allow(dead_code)]
#[derive(Copy, Clone)]
pub enum Translation {
    /// One-to-one address mapping
    Identity,
    /// Mapping with a specified offset
    Offset(usize),
}

/// Summary structure of memory region properties.
#[derive(Copy, Clone)]
pub struct AttributeFields {
    /// Attributes
    pub mem_attributes: MemAttributes,
    /// Permissions
    pub acc_perms: AccessPermissions,
    /// Disable executable code in this region
    pub execute_never: bool,
}

/// Types used for compiling the virtual memory layout of the kernel using address ranges.
///
/// Memory region descriptor.
///
/// Used to construct iterable kernel memory ranges.
pub struct TranslationDescriptor {
    /// Name of the region
    pub name: &'static str,
    /// Virtual memory range
    pub virtual_range: fn() -> RangeInclusive<usize>,
    /// Mapping translation
    pub physical_range_translation: Translation,
    /// Attributes
    pub attribute_fields: AttributeFields,
}

/// Type for expressing the kernel's virtual memory layout.
pub struct KernelVirtualLayout<const NUM_SPECIAL_RANGES: usize> {
    /// The last (inclusive) address of the address space.
    max_virt_addr_inclusive: usize,

    /// Array of descriptors for non-standard (normal cacheable DRAM) memory regions.
    inner: [TranslationDescriptor; NUM_SPECIAL_RANGES],
}

//--------------------------------------------------------------------------------------------------
// Public Implementations
//--------------------------------------------------------------------------------------------------

impl<const GRANULE_SIZE: usize> TranslationGranule<GRANULE_SIZE> {
    /// The granule's size.
    pub const SIZE: usize = Self::size_checked();

    /// The granule's shift, aka log2(size).
    pub const SHIFT: usize = Self::SIZE.trailing_zeros() as usize;

    const fn size_checked() -> usize {
        assert!(GRANULE_SIZE.is_power_of_two());

        GRANULE_SIZE
    }
}

impl<const AS_SIZE: usize> AddressSpace<AS_SIZE> {
    /// The address space size.
    pub const SIZE: usize = Self::size_checked();

    /// The address space shift, aka log2(size).
    pub const SIZE_SHIFT: usize = Self::SIZE.trailing_zeros() as usize;

    const fn size_checked() -> usize {
        assert!(AS_SIZE.is_power_of_two());

        // Check for architectural restrictions as well.
        Self::arch_address_space_size_sanity_checks();

        AS_SIZE
    }
}

impl Default for AttributeFields {
    fn default() -> AttributeFields {
        AttributeFields {
            mem_attributes: MemAttributes::CacheableDRAM,
            acc_perms: AccessPermissions::ReadWrite,
            execute_never: true,
        }
    }
}

/// Human-readable output of AttributeFields
impl fmt::Display for AttributeFields {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let attr = match self.mem_attributes {
            MemAttributes::CacheableDRAM => "C",
            MemAttributes::NonCacheableDRAM => "NC",
            MemAttributes::Device => "Dev",
        };

        let acc_p = match self.acc_perms {
            AccessPermissions::ReadOnly => "RO",
            AccessPermissions::ReadWrite => "RW",
        };

        let xn = if self.execute_never { "PXN" } else { "PX" };

        write!(f, "{: <3} {} {: <3}", attr, acc_p, xn)
    }
}

/// Human-readable output of a Descriptor.
impl fmt::Display for TranslationDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Call the function to which self.range points, and dereference the
        // result, which causes Rust to copy the value.
        let start = *(self.virtual_range)().start();
        let end = *(self.virtual_range)().end();
        let size = end - start + 1;

        // log2(1024)
        const KIB_SHIFT: u32 = 10;

        // log2(1024 * 1024)
        const MIB_SHIFT: u32 = 20;

        let (size, unit) = if (size >> MIB_SHIFT) > 0 {
            (size >> MIB_SHIFT, "MiB")
        } else if (size >> KIB_SHIFT) > 0 {
            (size >> KIB_SHIFT, "KiB")
        } else {
            (size, "Byte")
        };

        write!(
            f,
            "      {:#010x} - {:#010x} | {: >3} {} | {} | {}",
            start, end, size, unit, self.attribute_fields, self.name
        )
    }
}

impl<const NUM_SPECIAL_RANGES: usize> KernelVirtualLayout<{ NUM_SPECIAL_RANGES }> {
    /// Create a new instance.
    pub const fn new(max: usize, layout: [TranslationDescriptor; NUM_SPECIAL_RANGES]) -> Self {
        Self {
            max_virt_addr_inclusive: max,
            inner: layout,
        }
    }

    /// For a given virtual address, find and return the output address and
    /// corresponding attributes.
    ///
    /// If the address is not found in `inner`, return an identity mapped default for normal
    /// cacheable DRAM.
    pub fn virt_addr_properties(
        &self,
        virt_addr: usize,
    ) -> Result<(usize, AttributeFields), &'static str> {
        if virt_addr > self.max_virt_addr_inclusive {
            return Err("Address out of range");
        }

        for i in self.inner.iter() {
            if (i.virtual_range)().contains(&virt_addr) {
                let output_addr = match i.physical_range_translation {
                    Translation::Identity => virt_addr,
                    Translation::Offset(a) => a + (virt_addr - (i.virtual_range)().start()),
                };

                return Ok((output_addr, i.attribute_fields));
            }
        }

        Ok((virt_addr, AttributeFields::default()))
    }

    /// Print the kernel memory layout.
    pub fn print_layout(&self) {
        println!("[i] Kernel memory layout:"); //info!

        for i in self.inner.iter() {
            // for i in KERNEL_VIRTUAL_LAYOUT.iter() {
            println!("{}", i); //info!
        }
    }
}
