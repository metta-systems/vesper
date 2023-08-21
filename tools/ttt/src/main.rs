use {
    anyhow::{anyhow, Result},
    clap::{Arg, Command},
    colored::*,
    goblin::{
        elf::{program_header::ProgramHeader, Elf},
        error, Object,
    },
    machine::{
        memory::{
            mmu::{
                translation_table::interface::TranslationTable, AccessPermissions, AttributeFields,
                MemAttributes,
            },
            Address, Physical, Virtual,
        },
        platform::memory::{map, mmu::KernelGranule},
    },
    std::{fmt, iter::Map, path::Path},
};

// ttt /path/to/kernel.elf
fn main() -> Result<()> {
    let matches = Command::new("ttt - translation tables tool")
        .about("Patch kernel ELF file with calculated MMU mappings")
        .disable_version_flag(true)
        .arg(
            Arg::new("kernel")
                .long("kernel")
                .help("Path of the kernel ELF file to patch")
                .default_value("nucleus.elf"),
        )
        .get_matches();
    let kernel_elf_path = matches
        .get_one::<String>("kernel")
        .expect("kernel file must be specified");

    let kernel_elf = KernelElf::new(kernel_elf_path).unwrap();

    let platform = RaspberryPi::new(); // formerly BSP

    let translation_tables =
        /*machine::arch::aarch64::memory::mmu::translation_tables::*/TranslationTables::new();

    map_kernel_binary(&kernel_elf, translation_tables)?;

    patch_kernel_tables(kernel_elf_path, translation_tables, platform);
    patch_kernel_base_addr(kernel_elf_path, translation_tables, platform);
}

struct KernelElf {
    elf: Elf,
}

impl KernelElf {
    pub fn new(path: &Path) -> Result<Self> {
        let mut elf = Elf::new(path)?;
        Ok(Self { elf })
    }

    pub fn symbol_value(&self, symbol_name: &str) -> Result<u64> {
        let symbol = self
            .elf
            .syms
            .iter()
            .find(|sym| self.elf.strtab.get_at(sym.st_name) == Some(symbol_name))
            .ok_or_else(|| anyhow!("symbol {} not found", symbol_name))?;
        Ok(symbol.st_value)
    }

    pub fn segment_containing_virt_addr(&self, virt_addr: u64) -> Result<ProgramHeader> {
        for segment in self.elf.program_headers() {
            if segment.vm_range().contains(virt_addr) {
                return Ok(segment);
            }
        }
        Err(anyhow!(
            "virtual address {:#x} not in any segment",
            virt_addr
        ))
    }

    pub fn virt_to_phys(&self, virt_addr: u64) -> Result<u64> {
        let segment = self.segment_containing_virt_addr(virt_addr)?;
        let translation_offset = segment.p_vaddr.checked_sub(virt_addr);
        Ok(segment.p_paddr + translation_offset)
    }

    pub fn virt_to_file_offset(&self, virt_addr: u64) -> Result<u64> {
        let segment = self.segment_containing_virt_addr(virt_addr)?;
        let translation_offset = segment.p_vaddr.checked_sub(virt_addr);
        Ok(segment.p_offset + translation_offset)
    }

    pub fn sections_in_segment_as_string(&self, segment: &ProgramHeader) -> Result<String> {
        Ok(self
            .elf
            .section_headers()
            .iter()
            .filter(|section| segment.vm_range().contains(&section.sh_addr))
            .filter(|section| section.is_alloc())
            .sort(|a, b| a.sh_addr.cmp(&b.sh_addr))
            .map(|section| format!("{}", self.elf.strtab.get_at(section.sh_name)?))
            .collect::<Vec<_>>()
            .join(" "))
    }

    pub fn segment_acc_perms(segment: &ProgramHeader) -> Result<AccessPermissions> {
        if segment.is_read() && segment.is_write() {
            Ok(AccessPermissions::ReadWrite)
        } else if segment.is_read() {
            Ok(AccessPermissions::ReadOnly)
        } else {
            Err(anyhow!("Invalid segment access permissions"))
        }
    }

    pub fn generate_mapping_descriptors(&self) -> Result<Vec<MappingDescriptor>> {
        #[cfg(any(feature = "rpi3", feature = "rpi4"))]
        use machine::platform::raspberrypi::memory::mmu::KernelGranule;
        Ok(self
            .elf
            .program_headers()
            .iter()
            .filter(|segment| segment.is_alloc())
            .map(|segment| {
                // Assume each segment is page aligned.
                let size = segment.vm_range().len().align_up(KernelGranule::SIZE);
                let virt_start_addr = segment.p_vaddr;
                let phys_start_addr = segment.p_paddr;
                let virt_region = MemoryRegion::new(virt_start_addr, size, KernelGranule::SIZE)?;
                let phys_region = MemoryRegion::new(phys_start_addr, size, KernelGranule::SIZE)?;

                let attributes = AttributeFields {
                    mem_attributes: MemAttributes::CacheableDRAM,
                    acc_perms: Self::segment_acc_perms(segment)?,
                    execute_never: !segment.is_executable(),
                };

                MappingDescriptor {
                    name: self.sections_in_segment_as_string(&segment)?,
                    virt_region,
                    phys_region,
                    attributes,
                }
            })
            .collect())
    }
}

struct RaspberryPi {
    pub kernel_virt_addr_space_size: usize,
    pub virt_addr_of_kernel_tables: Address<Virtual>,
    pub virt_addr_of_phys_kernel_tables_base_addr: Address<Virtual>,
}

impl RaspberryPi {
    // attr_reader :kernel_granule, :kernel_virt_addr_space_size

    pub fn new(elf: &KernelElf) -> Result<Self> {
        Self {
            kernel_virt_addr_space_size: elf.symbol_value("__kernel_virt_addr_space_size")?
                as usize,
            virt_addr_of_kernel_tables: elf.symbol_value("KERNEL_TABLES")?,
            virt_addr_of_phys_kernel_tables_base_addr: elf
                .symbol_value("PHYS_KERNEL_TABLES_BASE_ADDR")?,
        }
    }

    pub fn phys_addr_of_kernel_tables(&self) -> Result<u64> {
        kernel_elf.virt_to_phys(self.virt_addr_of_kernel_tables)
    }

    pub fn kernel_tables_offset_in_file(&self) -> Result<u64> {
        kernel_elf.virt_to_file_offset(self.virt_addr_of_kernel_tables)
    }

    // def phys_kernel_tables_base_addr_offset_in_file
    // KERNEL_ELF.virt_addr_to_file_offset(@virt_addr_of_phys_kernel_tables_base_addr)
    // end

    pub fn phys_addr_space_end_page() -> Address<Physical> {
        map::END
    }
}

// # An array where each value is the start address of a Page.
// class MemoryRegion < Array
// def initialize(start_addr, size, granule_size)
// raise unless start_addr.aligned?(granule_size)
// raise unless size.positive?
// raise unless (size % granule_size).zero?
//
// num_pages = size / granule_size
// super(num_pages) do |i|
// (i * granule_size) + start_addr
// end
// end
// end
struct MemoryRegion {
    start: Address<Virtual>,
    size: usize,
    granularity: usize,
}

impl MemoryRegion {
    pub fn new(start: Address<Virtual>, size: usize, granularity: usize) -> Result<Self> {
        Ok(Self {
            start,
            size,
            granularity,
        })
    }
}

/// A container that describes a virt-to-phys region mapping.
struct MappingDescriptor {
    pub name: String,
    pub virt_region: MemoryRegion,
    pub phys_region: MemoryRegion,
    pub attributes: AttributeFields,
}

impl MappingDescriptor {
    pub fn print_header() {}
    pub fn print_divider() {}
}

impl fmt::Display for MappingDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MappingDescriptor")
        // def to_s
        // name = @name.ljust(self.class.max_section_name_length)
        // virt_start = @virt_region.first.to_hex_underscore(with_leading_zeros: true)
        // phys_start = @phys_region.first.to_hex_underscore(with_leading_zeros: true)
        // size = ((@virt_region.size * 65_536) / 1024).to_s.rjust(3)
        //
        // "#{name} | #{virt_start} | #{phys_start} | #{size} KiB | #{@attributes}"
        // end
    }
}

fn map_kernel_binary(kernel: &KernelElf, translation_tables: &mut TranslationTables) -> Result<()> {
    let mapping_descriptors = kernel.generate_mapping_descriptors()?;

    // Generate_mapping_descriptors updates the header being printed
    // with this call.So it must come afterwards.
    mapping_descriptors.print_header();

    // @todo use prettytable-rs

    let _ = mapping_descriptors.enumerate(|i, desc| {
        println!("{:>12} {}", "Generating".green().bold(), i);

        translation_tables.map_at(desc.virt_region, desc.phys_region, desc.attributes);
    });

    mapping_descriptors.print_divider();
    Ok(())
}

fn patch_kernel_tables(
    kernel_elf_path: &str,
    translation_tables: &TranslationTable,
    platform: &RaspberryPi,
) {
    println!(
        "{:>12} Kernel table struct at ELF file offset {}",
        "Patching".bold(),
        platform.kernel_tables_offset_in_file.to_hex_underscore()
    );

    // patch a file section with new data
    File.binwrite(
        kernel_elf_path,
        translation_tables.to_binary(),
        platform.kernel_tables_offset_in_file,
    );
}

fn patch_kernel_base_addr(
    kernel_elf_path: &str,
    translation_tables: &TranslationTable,
    platform: &RaspberryPi,
) {
    println!(
        "{:>12} Kernel tables physical base address start argument to value {} at ELF file offset {}",
        "Patching".green().bold(),
        translation_tables.phys_tables_base_addr.to_hex_underscore(),
        platform.phys_kernel_tables_base_addr_offset_in_file.to_hex_underscore()
    );

    // patch a file section with new data
    File.binwrite(
        kernel_elf_path,
        translation_tables.phys_tables_base_addr_binary,
        platform.phys_kernel_tables_base_addr_offset_in_file,
    );
}
