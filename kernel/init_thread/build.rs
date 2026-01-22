// Parse nucleus ELF and extract sections

use {
    build_print::info,
    build_rs::output,
    goblin::elf::{Elf, program_header::PT_LOAD, section_header::SHT_NOBITS},
    std::{
        env,
        fs::{self, File},
        io::Read,
        path::Path,
    },
};

fn main() {
    let kernel_elf_path = "../target/aarch64-unknown-none/release/kernel"; // must be passed-in as input?

    output::rerun_if_changed(kernel_elf_path);
    output::rerun_if_changed("build.rs");
    output::rustc_link_arg(
        format!("--script={}/init_thread.ld", env!("CARGO_MANIFEST_DIR")).as_ref(),
    );

    let out_dir = env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir);

    // Read kernel ELF
    let mut elf_bytes = Vec::new();
    File::open(kernel_elf_path)
        .expect("Failed to open kernel ELF - build kernel first with: cd ../kernel && cargo build --release")
        .read_to_end(&mut elf_bytes)
        .expect("Failed to read kernel ELF");

    // Parse with goblin
    let elf = Elf::parse(&elf_bytes).expect("Failed to parse kernel ELF");

    // Verify it's what we expect
    assert!(elf.is_64, "Kernel must be ELF64");
    assert_eq!(
        elf.header.e_machine,
        goblin::elf::header::EM_AARCH64,
        "Kernel must be AArch64"
    );

    // Extract section information
    let sections = extract_sections(&elf, &elf_bytes, out_path);

    // Generate Rust code
    generate_rust_code(&sections, &elf, out_path);
}

/// Extracted section with all metadata needed for loading
#[derive(Debug)]
struct ExtractedSection {
    name: String,
    /// Virtual address in kernel's address space (higher-half)
    virt_addr: u64,
    /// Size in memory
    mem_size: usize,
    /// Size in file (0 for BSS)
    file_size: usize,
    /// Required alignment
    alignment: u64,
    /// Permissions
    readable: bool,
    writable: bool,
    executable: bool,
    /// Is this a NOBITS section (BSS)?
    is_nobits: bool,
    /// Output binary file name (None for BSS)
    bin_file: Option<String>,
}

/// Exception vector table information
#[derive(Debug)]
struct VectorTableInfo {
    /// Virtual address of the vector table
    virt_addr: u64,
    /// Size of the vector table (should be 0x800 = 2048 bytes)
    size: usize,
    /// Alignment requirement (must be 2KB aligned for VBAR)
    alignment: u64,
}

/// All extracted kernel metadata
#[derive(Debug)]
struct KernelSections {
    /// Virtual base address (start of kernel in virtual memory)
    virt_base: u64,
    /// Loadable sections (text, rodata, data)
    load_sections: Vec<ExtractedSection>,
    /// BSS section (separate because it has no file content)
    bss_section: Option<ExtractedSection>,
    /// Exception vector table info
    vector_table: Option<VectorTableInfo>,
}

fn extract_sections(elf: &Elf, elf_bytes: &[u8], out_path: &Path) -> KernelSections {
    let mut load_sections = Vec::new();
    let mut bss_section = None;
    let mut vector_table = None;
    let mut virt_base = u64::MAX;

    // First pass: find virtual base from program headers
    for ph in &elf.program_headers {
        if ph.p_type == PT_LOAD {
            virt_base = virt_base.min(ph.p_vaddr);
        }
    }

    // Build a map of section names to their info
    let section_info: Vec<_> = elf
        .section_headers
        .iter()
        .filter_map(|sh| {
            let name = elf.shdr_strtab.get_at(sh.sh_name)?;
            Some((name, sh))
        })
        .collect();

    // Extract relevant sections
    for (name, sh) in &section_info {
        // Skip sections we don't care about
        if !matches!(*name, ".text" | ".rodata" | ".data" | ".bss") {
            continue;
        }

        let is_nobits = sh.sh_type == SHT_NOBITS;

        // Determine permissions from section flags
        let readable = true;
        let writable = sh.sh_flags & goblin::elf::section_header::SHF_WRITE as u64 != 0;
        let executable = sh.sh_flags & goblin::elf::section_header::SHF_EXECINSTR as u64 != 0;

        let section = ExtractedSection {
            name: name.to_string(),
            virt_addr: sh.sh_addr,
            mem_size: sh.sh_size as usize,
            file_size: if is_nobits { 0 } else { sh.sh_size as usize },
            alignment: sh.sh_addralign,
            readable,
            writable,
            executable,
            is_nobits,
            bin_file: None,
        };

        if is_nobits {
            bss_section = Some(section);
        } else {
            load_sections.push(section);
        }
    }

    // Try to find vectors via symbol
    vector_table = find_vector_table_from_symbols(elf);

    // Sort load sections by virtual address
    load_sections.sort_by_key(|s| s.virt_addr);

    // Extract binary content for each loadable section
    for section in &mut load_sections {
        let sh = section_info
            .iter()
            .find(|(name, _)| *name == section.name)
            .map(|(_, sh)| *sh)
            .unwrap();

        let start = sh.sh_offset as usize;
        let end = start + sh.sh_size as usize;
        let content = &elf_bytes[start..end];

        let bin_filename = format!("kernel_{}.bin", section.name.trim_start_matches('.'));
        let bin_path = out_path.join(&bin_filename);
        fs::write(&bin_path, content)
            .unwrap_or_else(|e| panic!("Failed to write {}: {}", bin_filename, e));

        section.bin_file = Some(bin_filename);

        info!(
            "Extracted {}: vaddr=0x{:016X}, size=0x{:X}, align={}, perms={}{}{}",
            section.name,
            section.virt_addr,
            section.mem_size,
            section.alignment,
            if section.readable { "R" } else { "-" },
            if section.writable { "W" } else { "-" },
            if section.executable { "X" } else { "-" },
        );
    }

    if let Some(ref bss) = bss_section {
        info!(
            "BSS section: vaddr=0x{:016X}, size=0x{:X}, align={}",
            bss.virt_addr, bss.mem_size, bss.alignment,
        );
    }

    KernelSections {
        virt_base,
        load_sections,
        bss_section,
        vector_table,
    }
}

/// Try to find vector table location from symbols
fn find_vector_table_from_symbols(elf: &Elf) -> Option<VectorTableInfo> {
    // Common symbol names for exception vectors
    const VECTOR_SYMBOL: &str = "__vectors";

    for sym in &elf.syms {
        if let Some(name) = elf.strtab.get_at(sym.st_name)
            && VECTOR_SYMBOL == name
        {
            info!(
                "Found vector table symbol '{}': vaddr=0x{:016X}, size=0x{:X}",
                name, sym.st_value, sym.st_size
            );

            // Verify 2KB alignment
            if sym.st_value & 0x7FF != 0 {
                info!(
                    "Vector table symbol at 0x{:016X} is not 2KB aligned!",
                    sym.st_value
                );
            }

            return Some(VectorTableInfo {
                virt_addr: sym.st_value,
                // If size is 0, assume standard size of 0x800
                size: if sym.st_size > 0 {
                    sym.st_size as usize
                } else {
                    0x800
                },
                alignment: 2048, // VBAR requirement
            });
        }
    }

    output::error("No vector table symbol found! Kernel must define  __vectors symbol");

    None
}

fn generate_rust_code(sections: &KernelSections, _elf: &Elf, out_path: &Path) {
    let mut code = String::new();

    // Header
    code.push_str(
        r#"// Auto-generated kernel section metadata and binary includes
// DO NOT EDIT - Generated by build.rs

#[allow(unused)]
use crate::{
    loader::{BssSectionMeta, KernelImageInfo, KernelSectionMeta, LoadableSection, VectorTableMeta},
    memory::MemoryPermissions
};

"#,
    );

    // Generate binary includes
    code.push_str("// ═══════════════════════════════════════════════════════════════\n");
    code.push_str("// Binary section data (included at compile time)\n");
    code.push_str("// ═══════════════════════════════════════════════════════════════\n\n");

    for section in &sections.load_sections {
        let const_name = format!(
            "KERNEL_{}_BIN",
            section.name.trim_start_matches('.').to_uppercase()
        );
        let bin_file = section.bin_file.as_ref().unwrap();

        code.push_str(&format!(
            "static {}: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{}\"));\n",
            const_name, bin_file
        ));
    }

    code.push('\n');

    // Generate section metadata constants
    code.push_str("// ═══════════════════════════════════════════════════════════════\n");
    code.push_str("// Section metadata\n");
    code.push_str("// ═══════════════════════════════════════════════════════════════\n\n");

    for section in &sections.load_sections {
        let const_name = format!(
            "{}_META",
            section.name.trim_start_matches('.').to_uppercase()
        );

        code.push_str(&format!(
            r#"pub const {}: KernelSectionMeta = KernelSectionMeta {{
    name: "{}",
    virt_addr: 0x{:016X},
    size: 0x{:X},
    alignment: 0x{:X},
    permissions: MemoryPermissions {{ readable: {}, writable: {}, executable: {} }},
}};

"#,
            const_name,
            section.name,
            section.virt_addr,
            section.mem_size,
            section.alignment,
            section.readable,
            section.writable,
            section.executable,
        ));
    }

    // BSS metadata
    if let Some(ref bss) = sections.bss_section {
        code.push_str(&format!(
            r#"pub const BSS_META: BssSectionMeta = BssSectionMeta {{
    virt_addr: 0x{:016X},
    size: 0x{:X},
    alignment: 0x{:X},
}};

"#,
            bss.virt_addr, bss.mem_size, bss.alignment,
        ));
    }

    // Vector table metadata
    code.push_str("// ═══════════════════════════════════════════════════════════════\n");
    code.push_str("// Exception vector table\n");
    code.push_str("// ═══════════════════════════════════════════════════════════════\n\n");

    if let Some(ref vectors) = sections.vector_table {
        code.push_str(&format!(
            r#"pub const VECTORS_META: VectorTableMeta = VectorTableMeta {{
    virt_addr: 0x{:016X},
    size: 0x{:X},
    alignment: 0x{:X},
}};

"#,
            vectors.virt_addr, vectors.size, vectors.alignment,
        ));
    }

    // Generate loadable sections array
    code.push_str("// ═══════════════════════════════════════════════════════════════\n");
    code.push_str("// Combined section array\n");
    code.push_str("// ═══════════════════════════════════════════════════════════════\n\n");

    code.push_str("static LOADABLE_SECTIONS: &[LoadableSection] = &[\n");
    for section in &sections.load_sections {
        let meta_name = format!(
            "{}_META",
            section.name.trim_start_matches('.').to_uppercase()
        );
        let data_name = format!(
            "KERNEL_{}_BIN",
            section.name.trim_start_matches('.').to_uppercase()
        );

        code.push_str(&format!(
            "    LoadableSection {{ meta: {}, data: {} }},\n",
            meta_name, data_name
        ));
    }
    code.push_str("];\n\n");

    // Generate main kernel info
    code.push_str(&format!(
        r#"/// Complete kernel image information
pub static KERNEL: KernelImageInfo = KernelImageInfo {{
    virt_base: 0x{:016X},
    sections: LOADABLE_SECTIONS,
    bss: {},
    vectors: {},
}};

"#,
        sections.virt_base,
        if sections.bss_section.is_some() {
            "Some(BSS_META)"
        } else {
            "None"
        },
        if sections.vector_table.is_some() {
            "Some(VECTORS_META)"
        } else {
            "None"
        },
    ));

    // Write generated code
    let dest_path = out_path.join("kernel_sections.rs");
    fs::write(&dest_path, code).expect("Failed to write generated code");

    info!("Generated: {}", dest_path.display());
    info!("Kernel virtual base: 0x{:016X}", sections.virt_base);
    if let Some(ref v) = sections.vector_table {
        info!(
            "Vector table:        0x{:016X} (size: 0x{:X})",
            v.virt_addr, v.size
        );
    }
}
