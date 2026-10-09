//! Host-side image building for Vesper `build.rs` scripts.
//!
//! - [`ElfImage::read`] extracts the loadable sections, `.bss`, symbols and
//!   the `.exports` table from an `AArch64` ELF file.
//! - [`generate_kernel`] emits the description of the nucleus a manifest
//!   names, which kickstart loads at boot (`KERNEL: libimage::KernelImage`).
//! - [`bundle_components`] emits the components a manifest lists as
//!   page-aligned segments in a `.components` link section plus their
//!   `libimage::ComponentImage` descriptions.

use {
    goblin::elf::{
        Elf,
        header::EM_AARCH64,
        section_header::{SHF_EXECINSTR, SHF_WRITE, SHT_NOBITS},
    },
    std::{
        env,
        fmt::Write as _,
        fs,
        path::{Path, PathBuf},
    },
};

/// Append formatted text to a `String` (which cannot fail).
macro_rules! emit {
    ($out:expr, $($arg:tt)*) => {
        $out.write_fmt(format_args!($($arg)*))
            .expect("formatting into a String cannot fail")
    };
}

pub const PAGE_SIZE: u64 = 4096;

/// Bytes reserved for an export's name in a `.exports` record; the record is
/// the NUL-padded name followed by the little-endian `u64` entry address.
/// `libobject::ppc_export!` emits this layout.
pub const EXPORT_NAME_BYTES: usize = 32;
const EXPORT_RECORD_BYTES: usize = EXPORT_NAME_BYTES + 8;

/// One extracted section.
#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub virt_addr: u64,
    pub size: u64,
    pub alignment: u64,
    pub writable: bool,
    pub executable: bool,
    /// File content (empty for `.bss`)
    pub data: Vec<u8>,
}

impl Section {
    fn tag(&self) -> String {
        self.name.trim_start_matches('.').to_uppercase()
    }

    fn permissions(&self) -> String {
        format!(
            "Permissions {{ readable: true, writable: {}, executable: {} }}",
            self.writable, self.executable
        )
    }
}

/// The parts of an ELF image the builders use.
#[derive(Debug)]
pub struct ElfImage {
    /// Lowest virtual address of any `PT_LOAD` segment
    pub virt_base: u64,
    /// `.text`, `.rodata`, `.data`, by address
    pub load_sections: Vec<Section>,
    pub bss: Option<Section>,
    /// `(name, address, size)` of every symbol
    pub symbols: Vec<(String, u64, u64)>,
    /// `(name, address)` records from the `.exports` section
    pub exports: Vec<(String, u64)>,
}

impl ElfImage {
    /// Read and parse an `AArch64` ELF64 file.
    pub fn read(path: &Path) -> Self {
        let bytes = fs::read(path)
            .unwrap_or_else(|error| panic!("cannot read ELF {}: {error}", path.display()));
        let elf = Elf::parse(&bytes)
            .unwrap_or_else(|error| panic!("cannot parse ELF {}: {error}", path.display()));
        assert!(elf.is_64, "{} must be ELF64", path.display());
        assert_eq!(
            elf.header.e_machine,
            EM_AARCH64,
            "{} must be AArch64",
            path.display()
        );

        let virt_base = elf
            .program_headers
            .iter()
            .filter(|header| header.p_type == goblin::elf::program_header::PT_LOAD)
            .map(|header| header.p_vaddr)
            .min()
            .unwrap_or(0);

        let mut load_sections = Vec::new();
        let mut bss = None;
        let mut exports = Vec::new();
        for header in &elf.section_headers {
            let Some(name) = elf.shdr_strtab.get_at(header.sh_name) else {
                continue;
            };
            let content = || {
                let start = usize::try_from(header.sh_offset).unwrap_or(usize::MAX);
                let end = start + usize::try_from(header.sh_size).unwrap_or(usize::MAX);
                bytes[start..end].to_vec()
            };
            if name == ".exports" {
                exports = parse_exports(&content(), path);
                continue;
            }
            if !matches!(name, ".text" | ".rodata" | ".data" | ".bss") || header.sh_size == 0 {
                continue;
            }
            let is_bss = header.sh_type == SHT_NOBITS;
            let section = Section {
                name: name.to_string(),
                virt_addr: header.sh_addr,
                size: header.sh_size,
                alignment: header.sh_addralign,
                writable: header.sh_flags & u64::from(SHF_WRITE) != 0,
                executable: header.sh_flags & u64::from(SHF_EXECINSTR) != 0,
                data: if is_bss { Vec::new() } else { content() },
            };
            if is_bss {
                bss = Some(section);
            } else {
                load_sections.push(section);
            }
        }
        load_sections.sort_by_key(|section| section.virt_addr);

        let symbols = elf
            .syms
            .iter()
            .filter_map(|symbol| {
                let name = elf.strtab.get_at(symbol.st_name)?;
                Some((name.to_string(), symbol.st_value, symbol.st_size))
            })
            .collect();

        Self {
            virt_base,
            load_sections,
            bss,
            symbols,
            exports,
        }
    }

    /// `(address, size)` of the symbol called `name`.
    pub fn symbol(&self, name: &str) -> Option<(u64, u64)> {
        self.symbols
            .iter()
            .find(|(symbol, _, _)| symbol == name)
            .map(|&(_, address, size)| (address, size))
    }
}

fn parse_exports(content: &[u8], path: &Path) -> Vec<(String, u64)> {
    assert_eq!(
        content.len() % EXPORT_RECORD_BYTES,
        0,
        "{}: malformed .exports section",
        path.display()
    );
    content
        .as_chunks::<EXPORT_RECORD_BYTES>()
        .0
        .iter()
        .map(|record| {
            let (name, address) = record.split_at(EXPORT_NAME_BYTES);
            let length = name
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(name.len());
            let name = std::str::from_utf8(&name[..length])
                .unwrap_or_else(|_| panic!("{}: export name is not UTF-8", path.display()));
            let address = u64::from_le_bytes(address.try_into().unwrap_or([0; 8]));
            (name.to_string(), address)
        })
        .collect()
}

/// The workspace's `target/<triple>` directory, under which the embedded
/// images (nucleus, userspace components) are found by [`find_artifact`].
pub fn artifact_dir() -> PathBuf {
    let target_dir = env::var_os("CARGO_TARGET_DIR").map_or_else(
        || {
            let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
            manifest
                .ancestors()
                .find(|dir| dir.join("Cargo.lock").exists())
                .unwrap_or(&manifest)
                .join("target")
        },
        PathBuf::from,
    );
    let triple = env::var("TARGET").unwrap_or_default();

    target_dir.join(triple)
}

/// Locate the built binary `binary` under [`artifact_dir`], trying
/// `release/<binary>`, then `debug/<binary>`, then `<binary>` itself, and
/// register the one found for rebuilds. The embedded image's profile is
/// independent of the profile the bundling crate is built or linted with.
///
/// # Panics
///
/// If none of the candidates exists.
pub fn find_artifact(binary: &str) -> PathBuf {
    let artifacts = artifact_dir();
    let candidates = [
        artifacts.join("release").join(binary),
        artifacts.join("debug").join(binary),
        artifacts.join(binary),
    ];
    let found = candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| {
            let tried: Vec<_> = candidates
                .iter()
                .map(|candidate| candidate.display().to_string())
                .collect();
            panic!("cannot find binary `{binary}`; tried {}", tried.join(", "))
        });
    rerun_if_changed(found);
    found.clone()
}

fn rerun_if_changed(path: &Path) {
    println!("cargo::rerun-if-changed={}", path.display());
}

fn out_dir() -> PathBuf {
    PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set for build scripts"))
}

// ─── Manifest ───────────────────────────────────────────────────────────

/// Load the image manifest `manifest` (a path relative to the building
/// crate) and register it for rebuilds.
///
/// A manifest describes what an image embeds: a `[nucleus]` table naming the
/// nucleus binary, and `[[component]]` entries for bundled userspace
/// components. Binaries are cargo binary names, located by [`find_artifact`].
fn load_manifest(manifest: &str) -> (PathBuf, toml::Table) {
    let path = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default()).join(manifest);
    rerun_if_changed(&path);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read manifest {}: {error}", path.display()));
    let table = text
        .parse()
        .unwrap_or_else(|error| panic!("invalid manifest {}: {error}", path.display()));
    (path, table)
}

// ─── Nucleus ────────────────────────────────────────────────────────────

/// Generate `$OUT_DIR/kernel_sections.rs` for the nucleus named by the
/// manifest's `[nucleus] binary`: the section blobs, their metadata, and
/// `KERNEL: libimage::KernelImage`.
pub fn generate_kernel(manifest: &str) {
    use minijinja::{Environment, context, syntax::SyntaxConfig};

    let (manifest_path, table) = load_manifest(manifest);
    let binary = table
        .get("nucleus")
        .and_then(|nucleus| nucleus.get("binary"))
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("{}: expected [nucleus] binary", manifest_path.display()));
    let image = ElfImage::read(&find_artifact(binary));
    let out = out_dir();

    let sections: Vec<_> = image
        .load_sections
        .iter()
        .map(|section| {
            let bin_file = format!("kernel_{}.bin", section.name.trim_start_matches('.'));
            fs::write(out.join(&bin_file), &section.data)
                .unwrap_or_else(|error| panic!("cannot write {bin_file}: {error}"));
            println!(
                "cargo::warning=info: Extracted {}: vaddr=0x{:016X}, size=0x{:X}, align={}, perms=R{}{}",
                section.name,
                section.virt_addr,
                section.size,
                section.alignment,
                if section.writable { "W" } else { "-" },
                if section.executable { "X" } else { "-" },
            );
            context! {
                name => match section.name.as_str() {
                    ".text" => "Nucleus code".to_string(),
                    ".rodata" => "Nucleus read-only data".to_string(),
                    ".data" => "Nucleus data".to_string(),
                    other => other.to_string(),
                },
                meta_name => format!("{}_META", section.tag()),
                bin_name => format!("KERNEL_{}_BIN", section.tag()),
                bin_file,
                virt_addr => section.virt_addr,
                size => section.size,
                align => section.alignment,
                r => true,
                w => section.writable,
                x => section.executable,
            }
        })
        .collect();

    let bss = image.bss.as_ref().expect("the nucleus has no .bss section");
    let (vectors, vectors_size) = image
        .symbol("__exception_vectors_start")
        .expect("the nucleus must define __exception_vectors_start");
    assert_eq!(vectors & 0x7FF, 0, "the vector table must be 2 KiB aligned");
    let (stack_virt_bottom, _) = image
        .symbol("__STACK_VIRT_BOTTOM")
        .expect("the nucleus must define __STACK_VIRT_BOTTOM");
    let (nucleus_set_anchor_virt, _) = image
        .symbol("nucleus_set_anchor")
        .expect("the nucleus must export nucleus_set_anchor");
    let (nucleus_set_platform_virt, _) = image
        .symbol("nucleus_set_platform")
        .expect("the nucleus must export nucleus_set_platform");

    let mut environment = Environment::new();
    environment.set_syntax(
        SyntaxConfig::builder()
            .trim_blocks(true)
            .build()
            .expect("valid template syntax"),
    );
    environment.add_filter("address", |value: usize| format!("0x{value:016X}"));
    environment.add_filter("hex", |value: usize| format!("0x{value:X}"));
    // minijinja renders booleans Python-style (`True`/`False`); this replaces
    // the builtin `bool` filter to emit Rust literals.
    environment.add_filter("bool", |value: bool| value.to_string());
    environment
        .add_template("sections", include_str!("kernel_sections.template.rs"))
        .expect("valid kernel template");
    let rendered = environment
        .get_template("sections")
        .and_then(|template| {
            template.render(context! {
                virt_addr => image.virt_base,
                sections => sections,
                bss => context! { virt_addr => bss.virt_addr, size => bss.size, align => bss.alignment },
                vectors => context! {
                    virt_addr => vectors,
                    size => if vectors_size > 0 { vectors_size } else { 0x800 },
                    align => 2048,
                },
                stack_virt_bottom,
                nucleus_set_anchor_virt,
                nucleus_set_platform_virt,
            })
        })
        .expect("kernel template renders");
    fs::write(out.join("kernel_sections.rs"), rendered).expect("cannot write kernel_sections.rs");
}

// ─── Privileged components ──────────────────────────────────────────────

/// One `PT_LOAD` segment of a privileged component.
#[derive(Debug)]
pub struct PrivilegedSegment {
    /// Offset from the image base (the component is linked at 0)
    pub offset: u64,
    /// In-memory size; bytes past `data` are zeroed
    pub mem_size: u64,
    pub writable: bool,
    pub executable: bool,
    /// File-backed content
    pub data: Vec<u8>,
}

/// The parts of a position-independent privileged component ELF the bundler uses.
#[derive(Debug)]
pub struct PrivilegedElf {
    pub segments: Vec<PrivilegedSegment>,
    /// `(offset, addend)` of every `R_AARCH64_RELATIVE` relocation
    pub relocations: Vec<(u64, u64)>,
    /// Offset of the operation table (`libirqchip::OPS_SYMBOL`)
    pub ops_offset: u64,
    /// Device tree `compatible` strings, in kind order
    pub compatible: Vec<String>,
}

impl PrivilegedElf {
    /// Read a privileged component linked with `drivers/privileged.ld`.
    ///
    /// # Panics
    ///
    /// If the file is not a PIE `AArch64` ELF linked at 0 with page-aligned
    /// segments, has any relocation other than `R_AARCH64_RELATIVE`, or lacks
    /// the operation table or the `compatible` list.
    pub fn read(path: &Path) -> Self {
        use goblin::elf::{
            program_header::{PF_W, PF_X, PT_INTERP, PT_LOAD},
            reloc::R_AARCH64_RELATIVE,
        };

        let display = path.display();
        let bytes =
            fs::read(path).unwrap_or_else(|error| panic!("cannot read ELF {display}: {error}"));
        let elf = Elf::parse(&bytes)
            .unwrap_or_else(|error| panic!("cannot parse ELF {display}: {error}"));
        assert!(elf.is_64, "{display} must be ELF64");
        assert_eq!(
            elf.header.e_machine, EM_AARCH64,
            "{display} must be AArch64"
        );
        assert!(
            !elf.program_headers
                .iter()
                .any(|header| header.p_type == PT_INTERP),
            "{display} must not request an interpreter"
        );
        assert!(
            elf.libraries.is_empty(),
            "{display} must not depend on shared libraries"
        );

        let segments: Vec<_> = elf
            .program_headers
            .iter()
            .filter(|header| header.p_type == PT_LOAD && header.p_memsz > 0)
            .map(|header| {
                assert_eq!(
                    header.p_vaddr % PAGE_SIZE,
                    0,
                    "{display}: segments must be page-aligned (use drivers/privileged.ld)"
                );
                let start = usize::try_from(header.p_offset).unwrap_or(usize::MAX);
                let end = start + usize::try_from(header.p_filesz).unwrap_or(usize::MAX);
                PrivilegedSegment {
                    offset: header.p_vaddr,
                    mem_size: header.p_memsz,
                    writable: header.p_flags & PF_W != 0,
                    executable: header.p_flags & PF_X != 0,
                    data: bytes[start..end].to_vec(),
                }
            })
            .collect();
        assert!(
            segments
                .iter()
                .all(|segment| !(segment.writable && segment.executable)),
            "{display}: a segment is both writable and executable"
        );

        assert_eq!(
            elf.dynrels.len() + elf.pltrelocs.len(),
            0,
            "{display}: only RELA dynamic relocations are supported"
        );
        let relocations = elf
            .dynrelas
            .iter()
            .map(|relocation| {
                assert_eq!(
                    relocation.r_type, R_AARCH64_RELATIVE,
                    "{display}: relocation at 0x{:X} is not R_AARCH64_RELATIVE",
                    relocation.r_offset
                );
                let addend = relocation.r_addend.unwrap_or(0);
                let addend = u64::try_from(addend)
                    .unwrap_or_else(|_| panic!("{display}: negative relocation addend {addend}"));
                assert!(
                    segments.iter().any(|segment| segment.writable
                        && (segment.offset..segment.offset + segment.mem_size)
                            .contains(&relocation.r_offset)),
                    "{display}: relocation at 0x{:X} is outside the writable segment",
                    relocation.r_offset
                );
                (relocation.r_offset, addend)
            })
            .collect();

        let ops_offset = elf
            .syms
            .iter()
            .find(|symbol| elf.strtab.get_at(symbol.st_name) == Some("IRQCHIP_OPS"))
            .map_or_else(
                || panic!("{display}: missing the IRQCHIP_OPS operation table"),
                |symbol| symbol.st_value,
            );

        let compatible = elf
            .section_headers
            .iter()
            .find(|header| elf.shdr_strtab.get_at(header.sh_name) == Some(".irqchip_compatible"))
            .map(|header| {
                let start = usize::try_from(header.sh_offset).unwrap_or(usize::MAX);
                let end = start + usize::try_from(header.sh_size).unwrap_or(usize::MAX);
                bytes[start..end]
                    .split(|&byte| byte == 0)
                    .filter(|entry| !entry.is_empty())
                    .map(|entry| {
                        std::str::from_utf8(entry)
                            .unwrap_or_else(|_| panic!("{display}: compatible is not UTF-8"))
                            .to_string()
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|compatible| !compatible.is_empty())
            .unwrap_or_else(|| panic!("{display}: missing the .irqchip_compatible list"));

        Self {
            segments,
            relocations,
            ops_offset,
            compatible,
        }
    }
}

/// Bundle the `[[privileged]]` entries of `manifest` (relative to the crate)
/// into `$OUT_DIR/privileged.rs`: one `pub static <NAME>:
/// libimage::PrivilegedImage` per component plus `pub static PRIVILEGED:
/// &[&PrivilegedImage]`, empty when the manifest lists none.
///
/// Each entry names the component (`name`, an identifier) and its cargo
/// `binary`, built with `drivers/privileged.ld` as a PIE.
pub fn bundle_privileged(manifest: &str) {
    let (manifest_path, table) = load_manifest(manifest);
    let out = out_dir();

    let mut code = String::from(
        "// Generated by vesper-image-build from the image manifest. Do not edit.\n\n\
         #[allow(unused)]\n\
         use libimage::{LoadableSection, Permissions, PrivilegedImage, Relocation, SectionMeta};\n\n",
    );
    let mut names = Vec::new();
    let entries = table
        .get("privileged")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for entry in &entries {
        let field = |key: &str| {
            entry
                .get(key)
                .and_then(toml::Value::as_str)
                .unwrap_or_else(|| {
                    panic!(
                        "{}: a [[privileged]] entry has no {key}",
                        manifest_path.display()
                    )
                })
        };
        let name = field("name");
        assert!(
            name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "{}: privileged component name `{name}` must be an identifier",
            manifest_path.display()
        );
        let binary = field("binary");
        let image = PrivilegedElf::read(&find_artifact(binary));
        let stem = name.to_uppercase();

        let mut segments = String::new();
        for (index, segment) in image.segments.iter().enumerate() {
            let bin_file = format!("privileged_{name}_{index}.bin");
            fs::write(out.join(&bin_file), &segment.data)
                .unwrap_or_else(|error| panic!("cannot write {bin_file}: {error}"));
            let blob = format!("{stem}_SEGMENT_{index}");
            emit!(
                code,
                "static {blob}: [u8; {len}] = *include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{bin_file}\"));\n",
                len = segment.data.len()
            );
            emit!(
                segments,
                "        LoadableSection {{ meta: SectionMeta {{ name: \"segment {index}\", virt_addr: 0x{offset:X}, size: 0x{size:X}, alignment: 0x{PAGE_SIZE:X}, permissions: Permissions {{ readable: true, writable: {writable}, executable: {executable} }} }}, data: &{blob} }},\n",
                offset = segment.offset,
                size = segment.mem_size,
                writable = segment.writable,
                executable = segment.executable,
            );
        }
        let relocations: String = image
            .relocations
            .iter()
            .map(|(offset, addend)| {
                format!("        Relocation {{ offset: 0x{offset:X}, addend: 0x{addend:X} }},\n")
            })
            .collect();
        let compatible: String = image
            .compatible
            .iter()
            .map(|entry| format!("\"{entry}\", "))
            .collect();
        println!(
            "cargo::warning=info: Bundled privileged {name} ({binary}): {} segments, {} relocations, drives [{}]",
            image.segments.len(),
            image.relocations.len(),
            image.compatible.join(", ")
        );
        emit!(
            code,
            "\n/// Privileged component `{name}` (`{binary}`).\n\
             pub static {stem}: PrivilegedImage = PrivilegedImage {{\n\
             \x20   name: \"{name}\",\n\
             \x20   compatible: &[{compatible}],\n\
             \x20   segments: &[\n{segments}    ],\n\
             \x20   relocations: &[\n{relocations}    ],\n\
             \x20   ops_offset: 0x{ops_offset:X},\n}};\n\n",
            ops_offset = image.ops_offset,
        );
        names.push(stem);
    }
    emit!(
        code,
        "/// Every bundled privileged component, in manifest order.\npub static PRIVILEGED: &[&PrivilegedImage] = &[{}];\n",
        names
            .iter()
            .map(|name| format!("&{name}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    fs::write(out.join("privileged.rs"), code).expect("cannot write privileged.rs");
}

// ─── Components ─────────────────────────────────────────────────────────

/// One `[[component]]` entry of a bundling manifest.
#[derive(Debug)]
struct ManifestEntry {
    /// Rust identifier stem for the generated statics
    name: String,
    /// Cargo binary name; the ELF is located by [`find_artifact`]
    binary: String,
    /// Whether the component is an active actor (`active = true`): it gets
    /// an entry where the loader may start its Threads. Components are passive
    /// by default — they only export procedures and get no entry.
    active: bool,
    /// Entry symbol of an active component (default `_start`)
    entry: Option<String>,
}

/// The `[[component]]` entries of a parsed manifest at `path`.
fn read_components(path: &Path, table: &toml::Table) -> Vec<ManifestEntry> {
    let entries = table
        .get("component")
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| panic!("{}: expected [[component]] entries", path.display()));
    entries
        .iter()
        .map(|entry| {
            let field = |key: &str| {
                entry
                    .get(key)
                    .and_then(toml::Value::as_str)
                    .map(str::to_string)
            };
            let name = field("name")
                .unwrap_or_else(|| panic!("{}: a component has no name", path.display()));
            assert!(
                name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{}: component name `{name}` must be an identifier",
                path.display()
            );
            ManifestEntry {
                binary: field("binary").unwrap_or_else(|| {
                    panic!("{}: component `{name}` has no binary", path.display())
                }),
                entry: field("entry"),
                active: entry
                    .get("active")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false),
                name,
            }
        })
        .collect()
}

/// Pad `data` with zeros to whole pages.
fn page_padded(mut data: Vec<u8>) -> Vec<u8> {
    let length = data
        .len()
        .next_multiple_of(usize::try_from(PAGE_SIZE).unwrap_or(4096));
    data.resize(length.max(usize::try_from(PAGE_SIZE).unwrap_or(4096)), 0);
    data
}

/// Bundle the `[[component]]` entries of `manifest` (relative to the crate)
/// into
/// `$OUT_DIR/components.rs`.
///
/// For each component this emits one page-aligned, page-padded static per
/// file-backed segment in the `.components` link section, and a
/// `pub static <NAME>: libimage::ComponentImage`, plus
/// `pub static COMPONENTS: &[&ComponentImage]`. The bundling image's linker
/// script must place `.components` page-aligned inside its retained image.
pub fn bundle_components(manifest: &str) {
    let (manifest_path, table) = load_manifest(manifest);
    let out = out_dir();

    let mut code = String::from(
        "// Generated by vesper-image-build from the component manifest. Do not edit.\n\n\
         #[allow(unused)]\n\
         use libimage::{ComponentImage, Export, LoadableSection, Permissions, SectionMeta};\n\n\
         /// Page-aligned storage for one component segment.\n\
         #[repr(C, align(4096))]\n\
         struct PageAligned<const N: usize>([u8; N]);\n\n",
    );
    let mut names = Vec::new();
    for component in read_components(&manifest_path, &table) {
        let image = ElfImage::read(&find_artifact(&component.binary));
        let stem = component.name.to_uppercase();

        let mut segments = String::new();
        for section in &image.load_sections {
            assert_eq!(
                section.virt_addr % PAGE_SIZE,
                0,
                "{}: {} must be page-aligned (use the userspace linker script)",
                component.binary,
                section.name
            );
            let data = page_padded(section.data.clone());
            let bin_file = format!(
                "component_{}_{}.bin",
                component.name,
                section.name.trim_start_matches('.')
            );
            fs::write(out.join(&bin_file), &data)
                .unwrap_or_else(|error| panic!("cannot write {bin_file}: {error}"));
            let blob = format!("{stem}_{}_BLOB", section.tag());
            emit!(
                code,
                "#[unsafe(link_section = \".components\")]\n\
                 static {blob}: PageAligned<{len}> = PageAligned(*include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{bin_file}\")));\n",
                len = data.len()
            );
            emit!(
                segments,
                "    LoadableSection {{ meta: SectionMeta {{ name: \"{name}\", virt_addr: 0x{addr:X}, size: 0x{size:X}, alignment: 0x{align:X}, permissions: {perms} }}, data: &{blob}.0 }},\n",
                name = section.name,
                addr = section.virt_addr,
                size = section.size,
                align = section.alignment.max(1),
                perms = section.permissions(),
            );
        }
        let bss = image.bss.as_ref().map_or_else(
            || "None".to_string(),
            |bss| {
                assert_eq!(
                    bss.virt_addr % PAGE_SIZE,
                    0,
                    "{}: .bss must be page-aligned (use the userspace linker script)",
                    component.binary
                );
                format!(
                    "Some(SectionMeta {{ name: \".bss\", virt_addr: 0x{:X}, size: 0x{:X}, alignment: 0x{:X}, permissions: {} }})",
                    bss.virt_addr,
                    bss.size,
                    bss.alignment.max(1),
                    bss.permissions()
                )
            },
        );
        assert!(
            component.active || component.entry.is_none(),
            "{}: `entry` is set but the component is not `active`",
            component.binary
        );
        let entry = if component.active {
            let symbol = component.entry.as_deref().unwrap_or("_start");
            let (address, _) = image
                .symbol(symbol)
                .unwrap_or_else(|| panic!("{}: missing entry symbol `{symbol}`", component.binary));
            format!("Some(0x{address:X})")
        } else {
            "None".to_string()
        };
        let exports: String = image
            .exports
            .iter()
            .map(|(name, address)| {
                format!("    Export {{ name: \"{name}\", address: 0x{address:X} }},\n")
            })
            .collect();
        emit!(
            code,
            "\n/// Component `{name}` (`{binary}`).\n\
             pub static {stem}: ComponentImage = ComponentImage {{\n\
             \x20   name: \"{name}\",\n\
             \x20   segments: &[\n{segments}    ],\n\
             \x20   bss: {bss},\n\
             \x20   entry: {entry},\n\
             \x20   exports: &[\n{exports}    ],\n}};\n\n",
            name = component.name,
            binary = component.binary,
        );
        names.push(stem);
    }
    emit!(
        code,
        "/// Every bundled component, in manifest order (a bundling image may use\n/// only the per-component statics).\n#[allow(dead_code)]\npub static COMPONENTS: &[&ComponentImage] = &[{}];\n",
        names
            .iter()
            .map(|name| format!("&{name}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    fs::write(out.join("components.rs"), code).expect("cannot write components.rs");
}
