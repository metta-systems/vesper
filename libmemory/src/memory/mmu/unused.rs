//--------------------------------------------------------------------------------------------------
// Laterrrr
//--------------------------------------------------------------------------------------------------

/// Architecture agnostic memory region translation types.
#[allow(dead_code)]
#[derive(Copy, Clone)]
pub enum Translation {
    /// One-to-one address mapping
    Identity,
    /// Mapping with a specified offset
    Offset(usize),
}

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

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
// Public Code
//--------------------------------------------------------------------------------------------------

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
