//! Device tree source (`.dts`-like) dump of an indexed tree, for debugging.
//!
//! Adapted from fdtdump (<https://github.com/rs-embedded/fdtdump>).

use {
    crate::{DeviceTree, FallibleIterator, Node, Prop, PropReader},
    core::fmt::{self, Write},
};

/// Write the header metadata and every node of `tree` to `out`.
///
/// # Errors
///
/// If writing to `out` fails.
pub fn dump(tree: &DeviceTree<'_, '_>, out: &mut impl Write) -> fmt::Result {
    let blob = tree.index().fdt();
    writeln!(out, "// magic:\t\t{:#x}", blob.magic())?;
    writeln!(out, "// totalsize:\t\t{:#x}", blob.totalsize())?;
    writeln!(out, "// version:\t\t{}", blob.version())?;
    writeln!(out, "// boot_cpuid_phys:\t{:#x}", blob.boot_cpuid_phys())?;
    for region in tree.reserved_memory() {
        writeln!(out, "/memreserve/ {:#x} {:#x};", region.start, region.size)?;
    }
    dump_node(out, &tree.index().root(), 0)
}

fn indent(out: &mut impl Write, depth: usize) -> fmt::Result {
    (0..depth).try_for_each(|_| out.write_str("  "))
}

fn dump_node(out: &mut impl Write, node: &Node<'_, '_, '_>, depth: usize) -> fmt::Result {
    let name = node.name().unwrap_or("");
    indent(out, depth)?;
    writeln!(out, "{} {{", if name.is_empty() { "/" } else { name })?;
    for prop in node.props() {
        dump_property(out, &prop, depth + 1)?;
    }
    for child in node.children() {
        dump_node(out, &child, depth + 1)?;
    }
    indent(out, depth)?;
    writeln!(out, "}};")
}

/// Whether `prop` is a list of non-empty strings.
fn is_string_list(prop: &Prop<'_, '_, '_>) -> bool {
    let mut strings = prop.iter_str();
    loop {
        match strings.next() {
            Ok(Some(string)) if !string.is_empty() => {}
            Ok(None) => return true,
            Ok(Some(_)) | Err(_) => return false,
        }
    }
}

fn dump_property(out: &mut impl Write, prop: &Prop<'_, '_, '_>, depth: usize) -> fmt::Result {
    indent(out, depth)?;
    write!(out, "{}", prop.name().unwrap_or("?"))?;
    let raw = prop.raw();
    if raw.is_empty() {
        return writeln!(out, ";");
    }
    out.write_str(" = ")?;
    if is_string_list(prop) {
        let mut strings = prop.iter_str();
        let mut separator = "";
        while let Ok(Some(string)) = strings.next() {
            write!(out, "{separator}\"{string}\"")?;
            separator = ", ";
        }
    } else if raw.len().is_multiple_of(size_of::<u32>()) {
        out.write_str("<")?;
        let mut separator = "";
        for cell in raw.as_chunks::<{ size_of::<u32>() }>().0 {
            write!(out, "{separator}{:#x}", u32::from_be_bytes(*cell))?;
            separator = " ";
        }
        out.write_str(">")?;
    } else {
        out.write_str("[")?;
        let mut separator = "";
        for byte in raw {
            write!(out, "{separator}{byte:02x}")?;
            separator = " ";
        }
        out.write_str("]")?;
    }
    writeln!(out, ";")
}
