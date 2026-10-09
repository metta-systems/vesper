#![no_std]

//! Image descriptions shared by the boot loader and component builders.
//!
//! `vesper-image-build` (a host-side `build.rs` helper) extracts these from
//! ELF files and generates `static` instances; the boot loader and test
//! builders read them. Three image kinds exist:
//!
//! - [`KernelImage`]: the nucleus, copied into fresh memory at boot and mapped
//!   in the kernel half;
//! - [`ComponentImage`]: a userspace component whose file-backed segments are
//!   page-aligned inside the bundling image, so they can be mapped in place as
//!   Frames, and whose `.bss` is allocated and mapped separately;
//! - [`PrivilegedImage`]: a position-independent EL1 component (e.g. an
//!   interrupt controller), copied, relocated and mapped in the kernel half.

use core::fmt;

pub const PAGE_SIZE: u64 = 4096;
/// [`PAGE_SIZE`] as a byte count.
pub const PAGE_BYTES: usize = 4096;

/// Access permissions of a section or mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
}

impl fmt::Display for Permissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{}",
            if self.readable { "R" } else { "-" },
            if self.writable { "W" } else { "-" },
            if self.executable { "X" } else { "-" }
        )
    }
}

/// Where a section lives and how it may be accessed.
#[derive(Debug, Clone, Copy)]
pub struct SectionMeta {
    /// Section name (for diagnostics)
    pub name: &'static str,
    /// Link-time virtual address
    pub virt_addr: u64,
    /// Size in memory, in bytes
    pub size: usize,
    /// Required alignment, in bytes
    pub alignment: u64,
    pub permissions: Permissions,
}

impl SectionMeta {
    /// Offset of the section from an image's virtual base.
    pub const fn offset_from_base(&self, virt_base: u64) -> u64 {
        self.virt_addr - virt_base
    }

    /// Physical address of the section in an image loaded at `phys_base`.
    pub const fn phys_addr(&self, phys_base: u64, virt_base: u64) -> u64 {
        phys_base + self.offset_from_base(virt_base)
    }

    /// Number of 4 KiB pages the section spans.
    pub const fn page_count(&self) -> usize {
        self.size.div_ceil(PAGE_BYTES)
    }
}

/// A file-backed section with its content.
#[derive(Debug)]
pub struct LoadableSection {
    pub meta: SectionMeta,
    pub data: &'static [u8],
}

/// The nucleus image: copied into fresh memory at boot, `.bss` zeroed.
#[derive(Debug)]
pub struct KernelImage {
    /// Lowest link-time virtual address of any loadable segment
    pub virt_base: u64,
    pub sections: &'static [LoadableSection],
    /// `.bss`: no content, zeroed at load
    pub bss: SectionMeta,
    /// Bottom of the kernel stack region (`__STACK_VIRT_BOTTOM`)
    pub stack_virt_bottom: u64,
    /// Exception vector table (for `VBAR_EL1`)
    pub vectors: SectionMeta,
}

impl KernelImage {
    /// Total memory the image needs (all sections and `.bss`), rounded up to
    /// whole pages.
    pub fn total_size(&self) -> usize {
        let section_ends = self
            .sections
            .iter()
            .map(|section| section.meta.virt_addr + section.meta.size as u64);
        let end = section_ends
            .chain(core::iter::once(self.bss.virt_addr + self.bss.size as u64))
            .max()
            .unwrap_or(self.virt_base);
        usize::try_from((end - self.virt_base).next_multiple_of(PAGE_SIZE)).unwrap_or(usize::MAX)
    }
}

/// A procedure a component exports for `AddressSpace.CreateInvocation`.
#[derive(Debug, Clone, Copy)]
pub struct Export {
    pub name: &'static str,
    /// Entry address in the component's own address space
    pub address: u64,
}

/// A userspace component bundled into a boot image.
///
/// Every segment's `data` is page-aligned and padded to whole pages inside
/// the bundling image, so its pages can be granted as Frames and mapped at
/// `meta.virt_addr` without copying.
#[derive(Debug)]
pub struct ComponentImage {
    pub name: &'static str,
    /// File-backed segments (`.text`, `.rodata`, `.data`), by address
    pub segments: &'static [LoadableSection],
    /// `.bss`, allocated and zeroed separately, if the component has one
    pub bss: Option<SectionMeta>,
    /// Address of the component's entry symbol, if it has one; a passive
    /// component only exports procedures
    pub entry: Option<u64>,
    pub exports: &'static [Export],
}

/// The `length` bytes of `memory` at image offset `offset`, if inside it.
fn span(memory: &mut [u8], offset: u64, length: usize) -> Option<&mut [u8]> {
    let start = usize::try_from(offset).ok()?;
    memory.get_mut(start..start.checked_add(length)?)
}

/// Why [`PrivilegedImage::load_into`] failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadError {
    /// The destination is smaller than the image
    TooSmall,
    /// A relocation targets a word outside the image
    BadRelocation,
}

/// One `R_AARCH64_RELATIVE` relocation of a [`PrivilegedImage`]: at load,
/// the 64-bit word at `offset` becomes `load_base + addend`.
#[derive(Debug, Clone, Copy)]
pub struct Relocation {
    /// Offset of the word from the image base
    pub offset: u64,
    pub addend: u64,
}

/// A privileged (EL1) component bundled into the boot image, e.g. an
/// interrupt controller (see `libirqchip`).
///
/// The image is position-independent and linked at 0: segment `virt_addr`s
/// are offsets from the load base. The loader copies the segments into fresh
/// memory, zeroes the rest of each segment, applies `relocations`, and maps
/// the result in the kernel half.
#[derive(Debug)]
pub struct PrivilegedImage {
    pub name: &'static str,
    /// Device tree `compatible` strings the component drives; the position of
    /// a string is the controller kind it is told about
    pub compatible: &'static [&'static str],
    /// Loadable segments, by offset: `data` holds the file-backed part,
    /// `meta.size` the in-memory size (the rest is zeroed)
    pub segments: &'static [LoadableSection],
    pub relocations: &'static [Relocation],
    /// Offset of the component's operation table
    pub ops_offset: u64,
}

impl PrivilegedImage {
    /// Memory the image spans from its base, rounded up to whole pages.
    pub fn total_size(&self) -> usize {
        let end = self
            .segments
            .iter()
            .map(|segment| segment.meta.virt_addr + segment.meta.size as u64)
            .max()
            .unwrap_or(0);
        usize::try_from(end.next_multiple_of(PAGE_SIZE)).unwrap_or(usize::MAX)
    }

    /// Lay the image out in `memory` (at least [`Self::total_size`] bytes,
    /// mapped at `load_base`): copy every segment, zero the rest, and apply
    /// the relocations.
    ///
    /// # Errors
    ///
    /// If `memory` is too small or a relocation falls outside it.
    pub fn load_into(&self, memory: &mut [u8], load_base: u64) -> Result<(), LoadError> {
        if memory.len() < self.total_size() {
            return Err(LoadError::TooSmall);
        }
        memory.fill(0);
        for segment in self.segments {
            span(memory, segment.meta.virt_addr, segment.data.len())
                .ok_or(LoadError::TooSmall)?
                .copy_from_slice(segment.data);
        }
        for relocation in self.relocations {
            span(memory, relocation.offset, size_of::<u64>())
                .ok_or(LoadError::BadRelocation)?
                .copy_from_slice(&(load_base + relocation.addend).to_le_bytes());
        }
        Ok(())
    }

    /// Whether the component drives the `compatible` string, and as which kind.
    pub fn kind_of(&self, compatible: &str) -> Option<u32> {
        self.compatible
            .iter()
            .position(|entry| *entry == compatible)
            .and_then(|kind| u32::try_from(kind).ok())
    }
}

impl ComponentImage {
    /// The address of the export called `name`.
    pub fn export(&self, name: &str) -> Option<u64> {
        self.exports
            .iter()
            .find(|export| export.name == name)
            .map(|export| export.address)
    }
}
