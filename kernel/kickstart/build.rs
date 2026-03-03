// Parse nucleus ELF and extract sections

use {
    build_print::info,
    build_rs::output,
    goblin::elf::{Elf, Sym, program_header::PT_LOAD, section_header::SHT_NOBITS},
    std::{
        env,
        fs::{self, File},
        io::Read,
        path::Path,
    },
};

fn main() {
    let kernel_elf_path = "../../target/aarch64-metta-none-eabi/release/nucleus"; // must be passed-in as input?

    output::rerun_if_changed(kernel_elf_path);
    output::rerun_if_changed("build.rs");
    output::rerun_if_changed("kernel_sections.template.rs");

    let out_dir = env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir);

    // Read kernel ELF
    let mut elf_bytes = Vec::new();
    File::open(kernel_elf_path)
        .expect("Failed to open kernel ELF - build kernel first with: just build")
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

    // Extract stack mapping information
    let stack_virt_bottom = stack_virt_bottom(&elf);

    // Generate Rust code
    generate_rust_code(&sections, stack_virt_bottom, out_path);
}

/// Extracted section with all metadata needed for loading
#[derive(Debug)]
struct ExtractedSection {
    name: String,
    /// Virtual address in kernel's address space (higher-half)
    virt_addr: u64,
    /// Size in memory
    mem_size: u64,
    /// Size in file (0 for BSS)
    // file_size: usize,
    /// Required alignment
    alignment: u64,
    /// Permissions
    readable: bool,
    writable: bool,
    executable: bool,
    /// Is this a NOBITS section (BSS)?
    // is_nobits: bool,
    /// Output binary file name (None for BSS)
    bin_file: Option<String>,
}

impl ExtractedSection {
    fn meta_name(&self) -> String {
        format!("{}_META", self.name.trim_start_matches('.').to_uppercase())
    }

    fn bin_name(&self) -> String {
        format!(
            "KERNEL_{}_BIN",
            self.name.trim_start_matches('.').to_uppercase()
        )
    }
}

/// Exception vector table information
#[derive(Debug)]
struct VectorTableInfo {
    /// Virtual address of the vector table
    virt_addr: u64,
    /// Size of the vector table (should be 0x800 = 2048 bytes)
    mem_size: u64,
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
        let writable = sh.sh_flags & u64::from(goblin::elf::section_header::SHF_WRITE) != 0;
        let executable = sh.sh_flags & u64::from(goblin::elf::section_header::SHF_EXECINSTR) != 0;

        let section = ExtractedSection {
            name: name.to_string(),
            virt_addr: sh.sh_addr,
            mem_size: sh.sh_size,
            // file_size: if is_nobits { 0 } else { sh.sh_size as usize },
            alignment: sh.sh_addralign,
            readable,
            writable,
            executable,
            // is_nobits,
            bin_file: None,
        };

        if is_nobits {
            bss_section = Some(section);
        } else {
            load_sections.push(section);
        }
    }

    // Try to find vectors via symbol
    let vector_table = find_vector_table_from_symbols(elf);

    // Sort load sections by virtual address
    load_sections.sort_by_key(|s| s.virt_addr);

    // Extract binary content for each loadable section
    for section in &mut load_sections {
        let sh = section_info
            .iter()
            .find(|(name, _)| *name == section.name)
            .map(|(_, sh)| *sh)
            .unwrap();

        let start = usize::try_from(sh.sh_offset).unwrap();
        let end = usize::try_from(sh.sh_offset + sh.sh_size).unwrap();
        let content = &elf_bytes[start..end];

        let bin_filename = format!("kernel_{}.bin", section.name.trim_start_matches('.'));
        let bin_path = out_path.join(&bin_filename);
        fs::write(&bin_path, content)
            .unwrap_or_else(|e| panic!("Failed to write {bin_filename}: {e}"));

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

fn find_symbol(elf: &Elf, symbol_name: &str) -> Option<Sym> {
    for sym in &elf.syms {
        if let Some(name) = elf.strtab.get_at(sym.st_name)
            && symbol_name == name
        {
            return Some(sym);
        }
    }

    None
}

fn map_section_name(name: &String) -> String {
    match name.as_ref() {
        ".text" => "Nucleus code".to_string(),
        ".rodata" => "Nucleus read-only data".to_string(),
        ".data" => "Nucleus data".to_string(),
        x => x.to_string(),
    }
}

/// Try to find vector table location from symbols
fn find_vector_table_from_symbols(elf: &Elf) -> Option<VectorTableInfo> {
    // Common symbol names for exception vectors
    const VECTOR_SYMBOL: &str = "__exception_vectors_start"; // From libexception::arch

    if let Some(sym) = find_symbol(elf, VECTOR_SYMBOL) {
        info!(
            "Found vector table symbol '{}': vaddr=0x{:016X}, size=0x{:X}",
            VECTOR_SYMBOL, sym.st_value, sym.st_size
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
            mem_size: if sym.st_size > 0 { sym.st_size } else { 0x800 },
            alignment: 2048, // VBAR requirement
        });
    }

    output::error("No vector table symbol found! Kernel must define  __vectors symbol");

    None
}

fn stack_virt_bottom(elf: &Elf) -> u64 {
    const STACK_VIRT_BOTTOM: &str = "__STACK_VIRT_BOTTOM";
    if let Some(sym) = find_symbol(elf, STACK_VIRT_BOTTOM) {
        return sym.st_value;
    }
    output::error("No stack bottom symbol found! Kernel must define  __STACK_VIRT_BOTTOM symbol");
    0
}

fn generate_rust_code(sections: &KernelSections, stack_virt_bottom: u64, out_path: &Path) {
    use minijinja::{Environment, context};

    let mut code = Environment::new();
    code.set_trim_blocks(true);
    code.add_filter("address", |v: usize| format!("0x{v:016X}"));
    code.add_filter("hex", |v: usize| format!("0x{v:X}"));
    code.add_template_owned(
        "sections",
        std::fs::read_to_string("kernel_sections.template.rs").unwrap(),
    )
    .unwrap();
    let tmpl = code.get_template("sections").unwrap();

    let sections_tmpl: Vec<_> = sections
        .load_sections
        .iter()
        .map(|section| {
            context! {
                name => map_section_name(&section.name),
                meta_name => section.meta_name(),
                bin_name => section.bin_name(),
                bin_file => section.bin_file,
                virt_addr => section.virt_addr,
                size => section.mem_size,
                align => section.alignment,
                r => section.readable,
                w => section.writable,
                x => section.executable,
            }
        })
        .collect();

    if sections.bss_section.is_none() {
        output::error("No BSS section for kernel, that's a bummer!");
    }

    let bss_section = sections.bss_section.as_ref().unwrap();

    let bss_tmpl = context! {
        virt_addr => bss_section.virt_addr,
        size => bss_section.mem_size,
        align => bss_section.alignment,
    };

    if sections.vector_table.is_none() {
        output::error("No vector table section for kernel, that's a bummer!");
    }

    let vector_table = sections.vector_table.as_ref().unwrap();

    let vector_tmpl = context! {
        virt_addr => vector_table.virt_addr,
        size => vector_table.mem_size,
        align => vector_table.alignment,
    };

    let context = context! {
        virt_addr => sections.virt_base,
        sections => sections_tmpl,
        bss => bss_tmpl,
        vectors => vector_tmpl,
        stack_virt_bottom,
    };

    // Write generated code
    let dest_path = out_path.join("kernel_sections.rs");
    fs::write(
        &dest_path,
        tmpl.render(context).expect("failed to render template"),
    )
    .expect("Failed to write generated code");

    info!("Generated: {}", dest_path.display());
    info!("Kernel virtual base: 0x{:016X}", sections.virt_base);
    if let Some(ref v) = sections.vector_table {
        info!(
            "Vector table:        0x{:016X} (size: 0x{:X})",
            v.virt_addr, v.mem_size
        );
    }
}
