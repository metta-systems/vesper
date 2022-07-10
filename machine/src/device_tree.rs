#![allow(dead_code)]

use {
    crate::{print, println},
    core::{alloc::Layout, mem::size_of, ptr::read_unaligned},
    fdt_rs::{
        base::{iters::StringPropIter, DevTree},
        error::{DevTreeError, Result as DevTreeResult},
        index::{DevTreeIndex, DevTreeIndexNode, DevTreeIndexProp},
        prelude::{PropReader, *},
    },
    shrinkwraprs::Shrinkwrap,
};

fn get_size_cell_tree_value<'a, 'i: 'a, 'dt: 'i>(
    node: DevTreeIndexNode<'a, 'i, 'dt>,
    name: &str,
) -> u32 {
    const DEFAULT: u32 = 1;

    let res: Result<_, DevTreeError> = node.props().try_find(|prop| Ok(prop.name()? == name));

    if !res.is_err() {
        if let Some(res) = res.unwrap() {
            return res.u32(0).unwrap_or(DEFAULT);
        }
    }

    while let Some(node) = node.parent() {
        let res: Result<_, DevTreeError> = node.props().try_find(|prop| Ok(prop.name()? == name));

        if res.is_err() {
            // @todo abort on error? because it's not a None, but an actual read error..
            continue;
        }

        if let Some(res) = res.unwrap() {
            return res.u32(0).unwrap_or(DEFAULT);
        }
    }

    DEFAULT
}

pub fn get_address_cells<'a, 'i: 'a, 'dt: 'i>(node: DevTreeIndexNode<'a, 'i, 'dt>) -> u32 {
    get_size_cell_tree_value(node, "#address-cells")
}

pub fn get_size_cells<'a, 'i: 'a, 'dt: 'i>(node: DevTreeIndexNode<'a, 'i, 'dt>) -> u32 {
    get_size_cell_tree_value(node, "#size-cells")
}

/// Uses DevTreeIndex implementation for simpler navigation.
/// This requires allocation of a single buffer, which is done at boot time via bump allocator.
/// This means we can only parse the tree after bump allocator is initialized.
#[derive(Shrinkwrap)]
pub struct DeviceTree<'a>(pub DevTreeIndex<'a, 'a>);

impl<'a> DeviceTree<'a> {
    pub fn layout(tree: DevTree<'a>) -> Result<Layout, DevTreeError> {
        DevTreeIndex::get_layout(&tree)
    }

    pub fn new(tree: DevTree<'a>, raw_slice: &'a mut [u8]) -> Result<Self, DevTreeError> {
        Ok(Self(DevTreeIndex::new(tree, raw_slice)?))
    }

    // @todo drop all the wrapper shenanigans and just export this one fn
    /// Iterate path separated by / starting from the root "/" and find props one by one.
    pub fn get_prop_by_path(&self, path: &str) -> Result<DevTreeIndexProp, DevTreeError> {
        let mut path = PathSplit::new(path);
        let mut node_iter = self.0.root().children();
        let mut node: Option<DevTreeIndexNode> = Some(self.0.root());
        if path.component().is_empty() {
            // Root "/"
            path.move_next();
        }
        while !path.is_finished() {
            let res: Result<_, DevTreeError> =
                node_iter.try_find(|node| Ok(node.name()? == path.component()));
            node = res?;
            if node.is_none() {
                return Err(DevTreeError::InvalidParameter("Invalid path")); // @todo
            }
            node_iter = node.as_ref().unwrap().children();
            path.move_next();
        }
        assert!(path.is_finished()); // tbd
        assert!(node.is_some());
        let mut prop_iter = node.unwrap().props();
        let res: Result<_, DevTreeError> =
            prop_iter.try_find(|prop| Ok(prop.name()? == path.component()));
        let prop = res?;
        if prop.is_none() {
            return Err(DevTreeError::InvalidParameter("Invalid path")); // @todo
        }
        Ok(prop.unwrap())
    }
}

/// Augment DevTreeIndexProp with a set of pairs accessor.
#[derive(Shrinkwrap)]
pub struct DeviceTreeProp<'a, 'i: 'a, 'dt: 'i>(DevTreeIndexProp<'a, 'i, 'dt>);

impl<'a, 'i: 'a, 'dt: 'i> DeviceTreeProp<'a, 'i, 'dt> {
    pub fn new(source: DevTreeIndexProp<'a, 'i, 'dt>) -> Self {
        Self(source)
    }

    pub fn payload_pairs_iter(&'a self) -> PayloadPairsIter<'a, 'i, 'dt> {
        let address_cells = get_address_cells(self.node());
        let size_cells = get_size_cells(self.node());

        // @todo boot this on 8Gb RasPi, because I'm not sure how it allocates memory regions there.
        println!(
            "Address cells: {}, size cells {}",
            address_cells, size_cells
        );

        PayloadPairsIter::new(&self.0, address_cells, size_cells)
    }
}

pub struct PayloadPairsIter<'a, 'i: 'a, 'dt: 'i> {
    prop: &'a DevTreeIndexProp<'a, 'i, 'dt>,
    total: usize,
    offset: usize,
    address_cells: u32,
    size_cells: u32,
}

impl<'a, 'i: 'a, 'dt: 'i> PayloadPairsIter<'a, 'i, 'dt> {
    pub fn new(
        prop: &'a DevTreeIndexProp<'a, 'i, 'dt>,
        address_cells: u32,
        size_cells: u32,
    ) -> Self {
        Self {
            prop,
            total: prop.length(),
            offset: 0usize,
            address_cells,
            size_cells,
        }
    }

    // @todo get rid of unwrap()s here
    fn prop_u32(&mut self, index: usize) -> u64 {
        self.prop.u32(index).unwrap().into()
    }

    // @todo get rid of unwrap()s here
    fn prop_u64(&mut self, index: usize) -> u64 {
        u64::from(self.prop.u32(index).unwrap()) << 32
            | u64::from(self.prop.u32(index + 1).unwrap())
    }

    fn read_pair(
        &mut self,
        size: usize,
        read_first: impl Fn(&mut Self, usize) -> u64,
        first_index: usize,
        read_second: impl Fn(&mut Self, usize) -> u64,
        second_index: usize,
    ) -> Option<(u64, u64)> {
        if self.offset + size > self.total {
            return None;
        }
        let result: (u64, u64) = (
            read_first(self, first_index),
            read_second(self, second_index),
        );
        self.offset += size;
        Some(result)
    }
}

impl<'a, 'i: 'a, 'dt: 'i> Iterator for PayloadPairsIter<'a, 'i, 'dt> {
    /// Return a pair of (address, size) values on each iteration.
    type Item = (u64, u64);

    fn next(&mut self) -> Option<Self::Item> {
        println!("Offset {}, total {}", self.offset, self.total);
        if self.offset >= self.total {
            return None;
        }
        const STEP: usize = size_of::<u32>();
        match (self.address_cells, self.size_cells) {
            (1, 1) => {
                const SIZE: usize = 8;
                self.read_pair(
                    SIZE,
                    Self::prop_u32,
                    self.offset / STEP,
                    Self::prop_u32,
                    self.offset / STEP + 1,
                )
            }
            (1, 2) => {
                const SIZE: usize = 12;
                self.read_pair(
                    SIZE,
                    Self::prop_u32,
                    self.offset / STEP,
                    Self::prop_u64,
                    self.offset / STEP + 1,
                )
            }
            (2, 1) => {
                const SIZE: usize = 12;
                self.read_pair(
                    SIZE,
                    Self::prop_u64,
                    self.offset / STEP,
                    Self::prop_u32,
                    self.offset / STEP + 2,
                )
            }
            (2, 2) => {
                const SIZE: usize = 16;
                self.read_pair(
                    SIZE,
                    Self::prop_u64,
                    self.offset / STEP,
                    Self::prop_u64,
                    self.offset / STEP + 2,
                )
            }
            _ => panic!("oooops"),
        }
    }
}

// See "2.2.3 Path Names" in DTSpec v0.3
// This is based on https://lib.rs/dtb implementation (c) Simon Prykhodko, MIT license.
struct PathSplit<'a> {
    path: &'a str,
    path_component: &'a str,
    index: usize,
    total: usize,
}

impl<'a> PathSplit<'a> {
    pub fn new(path: &'a str) -> PathSplit<'a> {
        let path = if let Some(p) = path.strip_suffix('/') {
            p
        } else {
            path
        };
        let mut split = PathSplit {
            path,
            path_component: "",
            index: 0,
            total: path.split('/').count(),
        };
        split.update();
        split
    }

    fn update(&mut self) {
        for (i, comp) in self.path.split('/').enumerate() {
            if i == self.index {
                self.path_component = comp;
                return;
            }
        }
    }

    pub fn component(&self) -> &'a str {
        self.path_component
    }

    pub fn level(&self) -> usize {
        self.index
    }

    pub fn is_finished(&self) -> bool {
        self.index >= self.total - 1
    }

    pub fn move_prev(&mut self) -> bool {
        if self.index > 0 {
            self.index -= 1;
            self.update();
            return true;
        }
        false
    }

    pub fn move_next(&mut self) -> bool {
        if self.index < self.total - 1 {
            self.index += 1;
            self.update();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{PathSplit, PayloadPairsIter};

    // PayloadPairsIter tests

    const BUF: [u32; 4] = [0x0000_0000, 0x2000_0000, 0x4000_0000, 0x8000_0000];

    #[test_case]
    fn parse_1_1_prop_correctly() {
        PayloadPairsIter
    }

    #[test_case]
    fn parse_1_2_prop_correctly() {
        PayloadPairsIter
    }

    #[test_case]
    fn parse_2_1_prop_correctly() {
        PayloadPairsIter
    }

    #[test_case]
    fn parse_2_2_prop_correctly() {
        PayloadPairsIter
    }

    // PathSplit tests

    #[test_case]
    fn test_single_level_path_split() {
        let mut path = PathSplit::new("/#address-cells");
        assert!(!path.is_finished());
        assert_eq!(path.level(), 0);
        assert_eq!(path.component(), "");

        assert_eq!(path.move_next(), true);

        assert!(path.is_finished());
        assert_eq!(path.level(), 1);
        assert_eq!(path.component(), "#address-cells");

        assert_eq!(path.move_next(), false);
    }

    #[test_case]
    fn test_multiple_level_path_split() {
        let mut path = PathSplit::new("/some/_other/#address-cells");
        assert!(!path.is_finished());
        assert_eq!(path.level(), 0);
        assert_eq!(path.component(), "");

        assert_eq!(path.move_next(), true);

        assert!(!path.is_finished());
        assert_eq!(path.level(), 1);
        assert_eq!(path.component(), "some");

        assert_eq!(path.move_next(), true);

        assert!(!path.is_finished());
        assert_eq!(path.level(), 2);
        assert_eq!(path.component(), "_other");

        assert_eq!(path.move_next(), true);

        assert!(path.is_finished());
        assert_eq!(path.level(), 3);
        assert_eq!(path.component(), "#address-cells");

        assert_eq!(path.move_next(), false);
    }
}

//=================================================================================================
// Dump the entire FDT
// From https://github.com/rs-embedded/fdtdump/blob/master/src/main.rs
//=================================================================================================

fn are_printable_strings(mut prop_iter: StringPropIter) -> bool {
    loop {
        match prop_iter.next() {
            Ok(Some(s_ref)) => {
                if s_ref.is_empty() {
                    return false;
                }
            }
            Ok(None) => return true,
            Err(_) => return false,
        }
    }
}

pub struct FdtDumper {
    pub indent: usize,
}

impl<'i, 'dt> FdtDumper {
    fn push_indent(&mut self) {
        for _ in 0..self.indent {
            print!("  ");
        }
    }

    fn dump_node_name(&mut self, name: &str) {
        self.push_indent();
        print!("{}", name);
        println!(" {{");
    }

    fn dump_node(&mut self, node: &DevTreeIndexNode) -> DevTreeResult<()> {
        let mut name = node.name()?;
        if name.is_empty() {
            name = "/";
        } else {
            name = node.name()?;
        }
        self.dump_node_name(name);
        Ok(())
    }

    fn dump_property(&mut self, prop: DevTreeIndexProp) -> DevTreeResult<()> {
        self.push_indent();

        print!("{}", prop.name()?);

        if prop.length() == 0 {
            println!(";");
            return Ok(());
        }
        print!(" = ");

        // Unsafe Ok - we're reinterpreting the data as expected.
        unsafe {
            // First try to parse as an array of strings
            if are_printable_strings(prop.iter_str()) {
                let mut iter = prop.iter_str();
                while let Some(s) = iter.next()? {
                    print!("\"{}\", ", s);
                }
                // let _ = self.dump.pop();
                // let _ = self.dump.pop();
            } else if prop.propbuf().len() % size_of::<u32>() == 0 {
                print!("<");
                for val in prop.propbuf().chunks_exact(size_of::<u32>()) {
                    // We use read_unaligned
                    #[allow(clippy::cast_ptr_alignment)]
                    let v = read_unaligned::<u32>(val.as_ptr() as *const u32);
                    let v = u32::from_be(v);
                    print!("{:#010x} ", v);
                }
                // let _ = self.dump.pop(); // Pop off extra space
                print!(">");
            } else {
                print!("[");
                for val in prop.propbuf() {
                    print!("{:02x} ", val);
                }
                // let _ = self.dump.pop(); // Pop off extra space
                print!("]");
            }
        }

        println!(";");
        Ok(())
    }

    pub fn dump_level(&mut self, node: &DevTreeIndexNode) -> DevTreeResult<()> {
        self.dump_node(node)?;
        self.indent += 1;
        for prop in node.props() {
            let _ = self.dump_property(prop)?;
        }
        for child in node.children() {
            let _ = self.dump_level(&child)?;
        }
        self.indent -= 1;
        self.push_indent();
        println!("}};");
        Ok(())
    }

    pub fn dump_metadata(&mut self, index: &DevTreeIndex) {
        let fdt = index.fdt();
        println!("// magic:\t\t{:#x}", fdt.magic());
        let s = fdt.totalsize();
        println!("// totalsize:\t\t{:#x} ({})", s, s);
        println!("// off_dt_struct:\t{:#x}", fdt.off_dt_struct());
        println!("// off_dt_strings:\t{:#x}", fdt.off_dt_strings());
        println!("// off_mem_rsvmap:\t{:#x}", fdt.off_mem_rsvmap());
        println!("// version:\t\t{:}", fdt.version());
        println!("// last_comp_version:\t{:}", fdt.last_comp_version());
        println!("// boot_cpuid_phys:\t{:#x}", fdt.boot_cpuid_phys());
        println!("// size_dt_strings:\t{:#x}", fdt.size_dt_strings());
        println!("// size_dt_struct:\t{:#x}", fdt.size_dt_struct());
        println!();
    }
}
