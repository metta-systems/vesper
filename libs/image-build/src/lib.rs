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

/// Where a component's first resource is mapped (`libimage::RESOURCE_BASE`).
const RESOURCE_BASE: u64 = 0x3c00_0000;

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
    /// Device tree `compatible` strings from a driver's `.driver_compatible`
    /// section (empty for other components)
    pub driver_compatible: Vec<String>,
    /// Entry addresses from a driver's `.driver_threads` section
    pub driver_threads: Vec<u64>,
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
        let mut driver_compatible = Vec::new();
        let mut driver_threads = Vec::new();
        for header in &elf.section_headers {
            let Some(name) = elf.shdr_strtab.get_at(header.sh_name) else {
                continue;
            };
            let content = || {
                let start = usize::try_from(header.sh_offset).unwrap_or(usize::MAX);
                let end = start + usize::try_from(header.sh_size).unwrap_or(usize::MAX);
                bytes[start..end].to_vec()
            };
            if name == ".driver_compatible" {
                driver_compatible = parse_strings(&content(), path);
                continue;
            }
            if name == ".driver_threads" {
                driver_threads = parse_addresses(&content(), path);
                continue;
            }
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
            driver_compatible,
            driver_threads,
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

/// The NUL-separated strings of a section such as `.driver_compatible`.
fn parse_strings(content: &[u8], path: &Path) -> Vec<String> {
    content
        .split(|&byte| byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            std::str::from_utf8(entry)
                .unwrap_or_else(|_| panic!("{}: a string is not UTF-8", path.display()))
                .to_string()
        })
        .collect()
}

/// The little-endian addresses of a section such as `.driver_threads`.
fn parse_addresses(content: &[u8], path: &Path) -> Vec<u64> {
    let (addresses, rest) = content.as_chunks::<8>();
    assert!(
        rest.is_empty(),
        "{}: malformed address table",
        path.display()
    );
    addresses
        .iter()
        .map(|bytes| u64::from_le_bytes(*bytes))
        .collect()
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

/// The profile whose images are embedded, named by its output directory
/// (`debug` or `release`): `VESPER_IMAGE_PROFILE` (exported by the Justfile),
/// `release` when unset. The embedded image's profile is independent of the
/// profile the bundling crate is built or linted with.
pub fn image_profile() -> String {
    println!("cargo::rerun-if-env-changed={IMAGE_PROFILE_VARIABLE}");
    env::var(IMAGE_PROFILE_VARIABLE).unwrap_or_else(|_| "release".to_string())
}

const IMAGE_PROFILE_VARIABLE: &str = "VESPER_IMAGE_PROFILE";

/// Locate the built binary `binary` in the [`image_profile`] directory under
/// [`artifact_dir`] and register it for rebuilds.
///
/// # Panics
///
/// If the binary has not been built with that profile.
pub fn find_artifact(binary: &str) -> PathBuf {
    let profile = image_profile();
    let path = artifact_dir().join(&profile).join(binary);
    assert!(
        path.is_file(),
        "cannot find binary `{binary}` at {}; build it with the `{profile}` profile first",
        path.display()
    );
    rerun_if_changed(&path);
    path
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
    let path = find_artifact(binary);
    let image = ElfImage::read(&path);
    assert!(
        image.load_sections.iter().any(|section| section.executable),
        "{}: the nucleus has no code; it was linked without the nucleus linker script \
         (a test build overwrites it under the same profile)",
        path.display()
    );
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
    /// The component's interface directory (`interfaces = "..."`, relative
    /// to the manifest): one `<name>_vN.if` per exported interface and an
    /// `imports.if` of the functions it calls
    interfaces: Option<PathBuf>,
    /// What Kickstart gives the one instance of a program it starts at boot
    /// (a component with a `priority`); `None` for components test builders load
    program: Option<Settings>,
    /// The `[[resource]]` entries the component maps read-only
    /// (`resources = [...]`), by name
    resources: Vec<String>,
}

/// A `[[resource]]` manifest entry: a data file bundled into the boot image
/// that Kickstart maps read-only into the components naming it.
struct ResourceEntry {
    name: String,
    file: PathBuf,
}

/// The `[[resource]]` entries of a parsed manifest at `path`.
fn read_resources(path: &Path, table: &toml::Table) -> Vec<ResourceEntry> {
    let entries = table
        .get("resource")
        .and_then(toml::Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    entries
        .iter()
        .map(|entry| {
            let field = |key: &str| entry.get(key).and_then(toml::Value::as_str);
            let name = field("name")
                .unwrap_or_else(|| panic!("{}: a resource has no name", path.display()));
            assert!(
                name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{}: resource name `{name}` must be an identifier",
                path.display()
            );
            let file = field("file")
                .unwrap_or_else(|| panic!("{}: resource `{name}` has no file", path.display()));
            ResourceEntry {
                name: name.to_string(),
                file: path.parent().unwrap_or(Path::new(".")).join(file),
            }
        })
        .collect()
}

/// What Kickstart gives each instance of a driver or program.
#[derive(Debug)]
struct Settings {
    /// Priority of the Time
    priority: u16,
    /// Budget the Time receives (default 1 s)
    budget_ns: u64,
    /// Size exponent of the RAM Untyped (default 64 KiB)
    memory_bits: u8,
    /// Whether the instance receives the `VideoCore` memory Untypeds (`video_memory = true`)
    video_memory: bool,
    /// Whether the instance is attached to the PL011 driver's serial channel
    /// (`serial = true`)
    serial: bool,
}

/// A `[[driver]]` manifest entry: an active component plus what Kickstart
/// gives each instance.
struct DriverEntry {
    component: ManifestEntry,
    settings: Settings,
}

/// Default budget of a driver's Time.
const DEFAULT_DRIVER_BUDGET_NS: u64 = 1_000_000_000;
/// Default size of a driver's RAM Untyped.
const DEFAULT_DRIVER_MEMORY: u64 = 64 * 1024;

/// The settings of the driver or program `name` from its manifest `entry`.
fn settings_of(path: &Path, kind: &str, name: &str, entry: &toml::Value) -> Settings {
    let number = |key: &str| entry.get(key).and_then(toml::Value::as_integer);
    let priority = number("priority")
        .and_then(|priority| u16::try_from(priority).ok())
        .unwrap_or_else(|| {
            panic!(
                "{}: {kind} `{name}` needs a priority (0-65535)",
                path.display()
            )
        });
    let budget_ns = number("budget_ns").map_or(DEFAULT_DRIVER_BUDGET_NS, |budget| {
        u64::try_from(budget)
            .unwrap_or_else(|_| panic!("{}: {kind} `{name}` has a negative budget", path.display()))
    });
    let memory = number("memory").map_or(DEFAULT_DRIVER_MEMORY, |memory| {
        u64::try_from(memory)
            .unwrap_or_else(|_| panic!("{}: {kind} `{name}` has negative memory", path.display()))
    });
    assert!(
        memory.is_power_of_two() && memory >= 4096,
        "{}: {kind} `{name}` memory must be a power of two of at least 4096 bytes",
        path.display()
    );
    Settings {
        priority,
        budget_ns,
        memory_bits: u8::try_from(memory.trailing_zeros()).unwrap_or(u8::MAX),
        video_memory: entry
            .get("video_memory")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        serial: entry
            .get("serial")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
    }
}

/// The `[[driver]]` entries of a parsed manifest at `path`.
fn read_drivers(path: &Path, table: &toml::Table) -> Vec<DriverEntry> {
    let entries = table
        .get("driver")
        .and_then(toml::Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    entries
        .iter()
        .map(|entry| {
            let text = |key: &str| {
                entry
                    .get(key)
                    .and_then(toml::Value::as_str)
                    .map(str::to_string)
            };
            let name =
                text("name").unwrap_or_else(|| panic!("{}: a driver has no name", path.display()));
            assert!(
                name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{}: driver name `{name}` must be an identifier",
                path.display()
            );
            DriverEntry {
                settings: settings_of(path, "driver", &name, entry),
                component: ManifestEntry {
                    binary: text("binary").unwrap_or_else(|| {
                        panic!("{}: driver `{name}` has no binary", path.display())
                    }),
                    entry: None,
                    interfaces: interface_directory(path, entry),
                    active: true,
                    program: None,
                    resources: Vec::new(),
                    name,
                },
            }
        })
        .collect()
}

/// The `[[component]]` entries of a parsed manifest at `path`. An entry with
/// a `priority` is a program: Kickstart starts one instance of it at boot,
/// like a driver without a device tree node.
fn read_components(path: &Path, table: &toml::Table) -> Vec<ManifestEntry> {
    let entries = table
        .get("component")
        .and_then(toml::Value::as_array)
        .map_or(&[][..], Vec::as_slice);
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
            let active = entry
                .get("active")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
            let program = entry
                .get("priority")
                .is_some()
                .then(|| settings_of(path, "program", &name, entry));
            assert!(
                program.is_none() || active,
                "{}: program `{name}` must be `active`",
                path.display()
            );
            ManifestEntry {
                binary: field("binary").unwrap_or_else(|| {
                    panic!("{}: component `{name}` has no binary", path.display())
                }),
                entry: field("entry"),
                interfaces: interface_directory(path, entry),
                active,
                program,
                resources: entry
                    .get("resources")
                    .and_then(toml::Value::as_array)
                    .map_or(&[][..], Vec::as_slice)
                    .iter()
                    .map(|resource| {
                        resource
                            .as_str()
                            .unwrap_or_else(|| {
                                panic!("{}: `{name}` resources are names", path.display())
                            })
                            .to_string()
                    })
                    .collect(),
                name,
            }
        })
        .collect()
}

/// The `interfaces` directory of a manifest `entry`, relative to the
/// manifest at `path`.
fn interface_directory(path: &Path, entry: &toml::Value) -> Option<PathBuf> {
    let directory = entry.get("interfaces").and_then(toml::Value::as_str)?;
    Some(path.parent().unwrap_or(Path::new(".")).join(directory))
}

/// Panic with the rendered diagnostics of a Meddle `failure`.
fn meddle_failure(failure: &libmeddle::Failure) -> ! {
    panic!("\n{}", failure.render(false))
}

/// The `interfaces: &[...]` and `imports: &[...]` initializers of
/// `component`, checking that `image` exports every function of the
/// interfaces it declares; `interface_directories` are those of every
/// component in the manifest, where the imported interfaces are found.
fn emit_interfaces(
    component: &ManifestEntry,
    image: &ElfImage,
    interface_directories: &[PathBuf],
) -> (String, String) {
    let Some(directory) = &component.interfaces else {
        return (String::new(), String::new());
    };
    rerun_if_changed(directory);
    let mut interfaces = String::new();
    for path in libmeddle::interface_files(directory) {
        rerun_if_changed(&path);
        let summary = libmeddle::summarize_interface(&path)
            .unwrap_or_else(|failure| meddle_failure(&failure));
        for index in 0..summary.functions.len() {
            let export = summary.export_name(index).unwrap_or_default();
            assert!(
                image.exports.iter().any(|(name, _)| *name == export),
                "{}: exports no `{export}` for interface `{}` (export it with `{}_export!`)",
                component.binary,
                summary.name,
                summary.name
            );
        }
        emit!(
            interfaces,
            "        InterfaceExport {{ name: \"{}\", type_code: 0x{:016x} }},\n",
            summary.name,
            summary.type_code
        );
    }
    let mut imports = String::new();
    let imports_file = directory.join(libmeddle::IMPORTS_FILE);
    if imports_file.is_file() {
        rerun_if_changed(&imports_file);
        let summaries = libmeddle::summarize_imports(&imports_file, interface_directories)
            .unwrap_or_else(|failure| meddle_failure(&failure));
        for summary in summaries {
            for index in summary.functions {
                emit!(
                    imports,
                    "        Import {{ interface: \"{}\", type_code: 0x{:016x}, function: {index}, links: &[{}], data_bytes: {}, placement_slots: {}, lends: {}, export: \"{}\" }},\n",
                    summary.interface.name,
                    summary.interface.type_code,
                    summary
                        .links
                        .iter()
                        .map(|link| match link {
                            libmeddle::LinkSummary::Property(name) =>
                                format!("Link::Property(\"{name}\")"),
                            libmeddle::LinkSummary::Compatible(name) =>
                                format!("Link::Compatible(\"{name}\")"),
                        })
                        .collect::<Vec<_>>()
                        .join(", "),
                    summary.interface.data_bytes,
                    summary.interface.placement_slots,
                    summary.interface.lends.get(index).copied().unwrap_or(false),
                    summary.interface.export_name(index).unwrap_or_default()
                );
            }
        }
    }
    (interfaces, imports)
}

/// Pad `data` with zeros to whole pages.
fn page_padded(mut data: Vec<u8>) -> Vec<u8> {
    let length = data
        .len()
        .next_multiple_of(usize::try_from(PAGE_SIZE).unwrap_or(4096));
    data.resize(length.max(usize::try_from(PAGE_SIZE).unwrap_or(4096)), 0);
    data
}

/// Bundle the `[[component]]` and `[[driver]]` entries of `manifest`
/// (relative to the crate) into
/// `$OUT_DIR/components.rs`.
///
/// For each component this emits one page-aligned, page-padded static per
/// file-backed segment in the `.components` link section, and a
/// `pub static <NAME>: libimage::ComponentImage`, plus
/// `pub static COMPONENTS: &[&ComponentImage]`. A driver is an active
/// component plus a `pub static <NAME>_DRIVER: libimage::DriverImage` with
/// the strings of its `.driver_compatible` section, the entries of its
/// `.driver_threads` section, its `priority`, its `budget_ns` and its
/// `memory` size, all listed in
/// `pub static DRIVERS: &[&DriverImage]`, followed by the programs: a
/// `[[component]]` entry with a `priority` (and `budget_ns`, `memory`,
/// `video_memory` like a driver's) gets a `DriverImage` with no `compatible`
/// strings, which Kickstart loads once without a device tree node. The
/// bundling image's linker
/// script must place `.components` page-aligned inside its retained image.
pub fn bundle_components(manifest: &str) {
    let (manifest_path, table) = load_manifest(manifest);
    let out = out_dir();

    let mut code = String::from(
        "// Generated by vesper-image-build from the component manifest. Do not edit.\n\n\
         #[allow(unused)]\n\
         use libimage::{ComponentImage, DriverImage, Export, Import, InterfaceExport, Link, LoadableSection, Permissions, ResourceMapping, SectionMeta};\n\n\
         /// Page-aligned storage for one component segment.\n\
         #[repr(C, align(4096))]\n\
         struct PageAligned<const N: usize>([u8; N]);\n\n",
    );
    let mut names = Vec::new();
    let components = read_components(&manifest_path, &table);
    let driver_entries = read_drivers(&manifest_path, &table);
    let resources = read_resources(&manifest_path, &table);
    let interface_directories: Vec<PathBuf> = components
        .iter()
        .chain(driver_entries.iter().map(|driver| &driver.component))
        .filter_map(|entry| entry.interfaces.clone())
        .collect();
    let mut programs = Vec::new();
    for component in &components {
        let image = ElfImage::read(&find_artifact(&component.binary));
        let stem = emit_component(
            &mut code,
            &out,
            component,
            &image,
            &interface_directories,
            &resources,
        );
        if let Some(settings) = &component.program {
            emit_driver_image(&mut code, &stem, component, settings, &image, "Program");
            programs.push(format!("&{stem}_DRIVER"));
        }
        names.push(stem);
    }
    let mut drivers = Vec::new();
    for driver in &driver_entries {
        let image = ElfImage::read(&find_artifact(&driver.component.binary));
        assert!(
            !image.driver_compatible.is_empty(),
            "{}: a driver declares its compatible strings with driver_compatible!",
            driver.component.binary
        );
        let stem = emit_component(
            &mut code,
            &out,
            &driver.component,
            &image,
            &interface_directories,
            &resources,
        );
        emit_driver_image(
            &mut code,
            &stem,
            &driver.component,
            &driver.settings,
            &image,
            "Driver",
        );
        names.push(stem.clone());
        drivers.push(format!("&{stem}_DRIVER"));
    }
    // Programs start after the drivers: they load without a device tree node.
    drivers.extend(programs);
    emit!(
        code,
        "/// Every bundled driver, in manifest order.\n#[allow(dead_code)]\npub static DRIVERS: &[&DriverImage] = &[{}];\n",
        drivers.join(", ")
    );
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

/// Emit `pub static <stem>_DRIVER: DriverImage` for `component`, an instance of
/// which Kickstart loads: a driver per device tree node it serves, a program
/// once (its `compatible` list is empty).
fn emit_driver_image(
    code: &mut String,
    stem: &str,
    component: &ManifestEntry,
    settings: &Settings,
    image: &ElfImage,
    what: &str,
) {
    let compatible: String = image
        .driver_compatible
        .iter()
        .map(|entry| format!("\"{entry}\", "))
        .collect();
    emit!(
        code,
        "\n/// {what} `{name}` (`{binary}`){list}.\n\
         pub static {stem}_DRIVER: DriverImage = DriverImage {{\n\
         \x20   component: &{stem},\n\
         \x20   compatible: &[{compatible}],\n\
         \x20   priority: 0x{priority:X},\n\
         \x20   budget_ns: {budget},\n\
         \x20   memory_bits: {memory_bits},\n\
         \x20   video_memory: {video_memory},\n\
         \x20   serial: {serial},\n\
         \x20   threads: &[{threads}],\n}};\n",
        name = component.name,
        binary = component.binary,
        list = if image.driver_compatible.is_empty() {
            String::new()
        } else {
            format!(", for `{}`", image.driver_compatible.join(", "))
        },
        priority = settings.priority,
        budget = settings.budget_ns,
        memory_bits = settings.memory_bits,
        video_memory = settings.video_memory,
        serial = settings.serial,
        threads = image
            .driver_threads
            .iter()
            .map(|entry| format!("0x{entry:X}, "))
            .collect::<String>(),
    );
}

/// Emit the segment statics and the `ComponentImage` static of `component`
/// (read as `image`) into `code`; the static's name.
fn emit_component(
    code: &mut String,
    out: &Path,
    component: &ManifestEntry,
    image: &ElfImage,
    interface_directories: &[PathBuf],
    resources: &[ResourceEntry],
) -> String {
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
    let mut mappings = String::new();
    let mut resource_address = RESOURCE_BASE;
    for name in &component.resources {
        let resource = resources
            .iter()
            .find(|resource| resource.name == *name)
            .unwrap_or_else(|| {
                panic!(
                    "{}: names the resource `{name}`, which no [[resource]] defines",
                    component.binary
                )
            });
        rerun_if_changed(&resource.file);
        let content = fs::read(&resource.file).unwrap_or_else(|error| {
            panic!("cannot read resource {}: {error}", resource.file.display())
        });
        let size = content.len();
        let data = page_padded(content);
        let bin_file = format!("component_{}_resource_{name}.bin", component.name);
        fs::write(out.join(&bin_file), &data)
            .unwrap_or_else(|error| panic!("cannot write {bin_file}: {error}"));
        let blob = format!("{stem}_RESOURCE_{}_BLOB", name.to_uppercase());
        emit!(
            code,
            "#[unsafe(link_section = \".components\")]\n\
             static {blob}: PageAligned<{len}> = PageAligned(*include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{bin_file}\")));\n",
            len = data.len()
        );
        emit!(
            segments,
            "    LoadableSection {{ meta: SectionMeta {{ name: \"{name}\", virt_addr: 0x{resource_address:X}, size: 0x{size:X}, alignment: 0x1000, permissions: Permissions {{ readable: true, writable: false, executable: false }} }}, data: &{blob}.0 }},\n",
        );
        emit!(
            mappings,
            "    ResourceMapping {{ name: \"{name}\", virt_addr: 0x{resource_address:X}, size: 0x{size:X} }},\n",
        );
        resource_address += data.len() as u64;
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
    let (interfaces, imports) = emit_interfaces(component, image, interface_directories);
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
         \x20   exports: &[\n{exports}    ],\n\
         \x20   interfaces: &[\n{interfaces}    ],\n\
         \x20   imports: &[\n{imports}    ],\n\
         \x20   resources: &[\n{mappings}    ],\n}};\n\n",
        name = component.name,
        binary = component.binary,
    );
    stem
}
