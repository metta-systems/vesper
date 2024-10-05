# Translation Tables Tool (ttt)

A Rust-based tool for generating and patching ARM64 translation tables into kernel ELF files for higher-half kernel mapping.

## Overview

This tool reads a kernel ELF file, generates pre-populated MMU translation tables based on the kernel's loadable segments, and patches these tables back into the ELF file. This enables the kernel to use virtual memory with higher-half addressing right from the start.

## Features

- **ELF Parsing**: Reads kernel ELF files and extracts symbol information, segment layouts, and section details
- **Translation Table Generation**: Creates ARM64 AArch64 two-level translation tables (Level 2 + Level 3) with 64 KiB granule size
- **Memory Mapping**: Maps kernel segments (code, data, stack) from physical to virtual addresses with appropriate permissions
- **ELF Patching**: Writes generated translation tables back into the kernel ELF file at the correct offsets
- **Higher-Half Kernel Support**: Enables kernel code and data to be accessed via high virtual addresses (typically 0xFFFF_FFFF_xxxx_xxxx)

## Architecture

### Translation Table Structure

The tool generates a two-level page table hierarchy for ARM64:

- **Level 2 Table**: Contains table descriptors pointing to level 3 tables (512 MiB blocks)
- **Level 3 Tables**: Contains page descriptors for 64 KiB pages

### Memory Attributes

The tool supports standard ARM64 memory attributes:

- **CacheableDRAM**: Normal cacheable memory (for code and data)
- **NonCacheableDRAM**: Non-cacheable memory
- **Device**: Device memory (for MMIO)

### Access Permissions

- **ReadOnly**: Read-only access (for code sections)
- **ReadWrite**: Read-write access (for data sections)

### Execute Permissions

- **execute_never**: Controls whether memory can be executed (PXN bit)

## Required ELF Symbols

The kernel ELF file must define these symbols:

- `__kernel_virt_addr_space_size`: Size of the kernel's virtual address space
- `KERNEL_TABLES`: Virtual address where translation tables are located in the kernel
- `PHYS_KERNEL_TABLES_BASE_ADDR`: Variable to store the physical base address of the tables

## Usage

```bash
# Build the tool
cargo build --release

# Run on a kernel ELF file
./target/release/ttt --kernel path/to/kernel.elf

# Or with default (kernel.elf in current directory)
./target/release/ttt
```

## Algorithm

1. **Load ELF File**: Parse the kernel ELF and extract symbols and segments
2. **Read Configuration**: Extract platform configuration from ELF symbols
3. **Generate Descriptors**: Create mapping descriptors for each loadable segment
4. **Initialize Tables**: Create level 2 and level 3 table structures
5. **Map Segments**: Generate page descriptors for each 64 KiB page in each segment
6. **Serialize Tables**: Convert in-memory tables to binary format (little-endian)
7. **Patch ELF**: Write tables to the KERNEL_TABLES location in the ELF file
8. **Patch Base Address**: Write the physical base address to PHYS_KERNEL_TABLES_BASE_ADDR

## Implementation Details

### Page Descriptor Format (Level 3)

```
Bits [63:54]: UXN, PXN (execute-never bits)
Bits [53:48]: Reserved
Bits [47:16]: Output address (physical address >> 16)
Bits [15:12]: Reserved
Bits [11:10]: Access Flag (AF)
Bits [9:8]:   Shareability (SH)
Bits [7:6]:   Access Permissions (AP)
Bits [4:2]:   Memory Attributes Index (AttrIndx)
Bit  [1]:     Type (1 = Page)
Bit  [0]:     Valid (1 = Valid)
```

### Table Descriptor Format (Level 2)

```
Bits [63:48]: Reserved
Bits [47:16]: Next level table address (physical address >> 16)
Bits [15:2]:  Reserved
Bit  [1]:     Type (1 = Table)
Bit  [0]:     Valid (1 = Valid)
```

## Differences from Ruby Implementation

This Rust implementation is based on the Ruby reference implementation from [rust-raspi3-OS-tutorials](https://github.com/berkus/rust-raspi3-OS-tutorials/tree/master/16_virtual_mem_part4_higher_half_kernel/tools/translation_table_tool) with these improvements:

- **Type Safety**: Uses Rust's type system to prevent errors
- **Memory Safety**: No manual memory management bugs
- **Better Error Handling**: Comprehensive error messages with `anyhow`
- **Modern ELF Parsing**: Uses the `goblin` crate for robust ELF handling
- **Standalone**: Doesn't require external dependencies on kernel code

## Binary Layout in ELF

The translation tables are stored in the ELF file in this order:

1. **Level 3 Tables**: `num_tables * 8192 * 8` bytes (64 KiB per table)
2. **Level 2 Table**: `num_tables * 8` bytes

The `KERNEL_TABLES` symbol points to the start of the level 3 tables. The level 2 table starts immediately after the level 3 tables.

## Example Output

```
Translation Tables Tool
Kernel ELF: kernel.elf
     Loading Kernel ELF file
     Loading Platform configuration
        Info Kernel virtual address space: 1024 MiB
Initializing Translation table structures

     Mapping Kernel segments:

.text .rodata              | 0xffff_ffff_0008_0000 | 0x00_0008_0000 |     640 KiB | C   RO PX 
.data .bss                 | 0xffff_ffff_000c_0000 | 0x00_000c_0000 |     256 KiB | C   RW PXN

    Patching Kernel translation tables at ELF file offset 0x100000
    Patching Kernel tables physical base address to 0x00_0010_0000 at ELF file offset 0x100008

    Finished in 0.02s
```

## Dependencies

- `anyhow`: Error handling
- `bytes`: Byte buffer manipulation
- `clap`: Command-line argument parsing
- `colored`: Colored terminal output
- `goblin`: ELF file parsing
- `prettytable-rs`: Table formatting (for future use)

## License

Same as the parent Vesper project (MIT OR Apache-2.0).
