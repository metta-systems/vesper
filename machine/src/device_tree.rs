use {
    core::alloc::Layout,
    fdt_rs::{
        base::DevTree,
        error::DevTreeError,
        index::{
            iters::DevTreeIndexNodeSiblingIter,
            {DevTreeIndex, DevTreeIndexNode, DevTreeIndexProp},
        },
        prelude::{FallibleIterator, PropReader},
    },
    shrinkwraprs::Shrinkwrap,
};

/// Uses DevTreeIndex implementation for simpler navigation.
/// This requires allocation of a single buffer, which is done at boot time via bump allocator.
/// This means we can only parse the tree after bump allocator is initialized.
#[derive(Shrinkwrap)]
pub struct DeviceTree<'a>(DevTreeIndex<'a, 'a>);

impl<'a> DeviceTree<'a> {
    pub fn layout(tree: DevTree<'a>) -> Result<Layout, DevTreeError> {
        Ok(DevTreeIndex::get_layout(&tree)?)
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
            node = node_iter
                .try_find::<_, _, DevTreeError>(|node| Ok(node.name()? == path.component()))?;
            if node.is_none() {
                return Err(DevTreeError::InvalidParameter("Invalid path")); // @todo
            }
            node_iter = node.as_ref().unwrap().children();
            path.move_next();
        }
        assert!(path.is_finished()); // tbd
        assert!(node.is_some());
        let mut prop_iter = node.unwrap().props();
        let prop = prop_iter
            .try_find::<_, _, DevTreeError>(|prop| Ok(prop.name()? == path.component()))?;
        if prop.is_none() {
            return Err(DevTreeError::InvalidParameter("Invalid path")); // @todo
        }
        Ok(prop.unwrap())
    }

    // // @todo boot this on 8Gb RasPi, because I'm not sure how it allocates memory regions there.
    // println!("Address cells: {}, size cells {}", address_cells, size_cells);
    //
    // let mem_prop = device_tree -- node
    // .props()
    // -- node with property named "device_type" and value "memory"
    // .find(|p| Ok(p.name()? == "device_type" && p.str()? == "memory"))
    // .unwrap()
    // .expect("Unable to find memory node.");
    // let mem_node = mem_prop.node();
    // // let parent_node = mem_node.parent_node();
}

/// Augment DevTreeIndexProp with set of pairs accessor.
#[derive(Shrinkwrap)]
pub struct DeviceTreeProp<'a, 'i: 'a, 'dt: 'i>(DevTreeIndexProp<'a, 'i, 'dt>);

impl<'a, 'i: 'a, 'dt: 'i> DeviceTreeProp<'a, 'i, 'dt> {
    pub fn new(source: DevTreeIndexProp<'a, 'i, 'dt>) -> Self {
        Self(source)
    }

    pub fn payload_pairs_iter(
        &'a self,
        address_cells: u32,
        size_cells: u32,
    ) -> PayloadPairsIter<'a, 'i, 'dt> {
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
}

impl<'a, 'i: 'a, 'dt: 'i> Iterator for PayloadPairsIter<'a, 'i, 'dt> {
    /// Return a pair of (address, size) values on each iteration.
    type Item = (u64, u64);
    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.total {
            // @todo check for sufficient space for the following read or the reads below may fail!
            return None;
        }
        Some(match (self.address_cells, self.size_cells) {
            (1, 1) => {
                let result: Self::Item = (
                    self.prop.u32(self.offset / 8).unwrap().into(),
                    self.prop.u32(self.offset / 8 + 1).unwrap().into(),
                );
                self.offset += 8;
                result
            }
            (1, 2) => {
                let result: Self::Item = (
                    self.prop.u32(self.offset / 12).unwrap().into(),
                    u64::from(self.prop.u32(self.offset / 12 + 1).unwrap()) << 32
                        | u64::from(self.prop.u32(self.offset / 12 + 2).unwrap()),
                );
                self.offset += 12;
                result
            }
            (2, 1) => {
                let result: Self::Item = (
                    self.prop.u64(self.offset / 12).unwrap(),
                    self.prop.u32(self.offset / 12 + 2).unwrap().into(),
                );
                self.offset += 12;
                result
            }
            (2, 2) => {
                let result: Self::Item = (
                    self.prop.u64(self.offset / 16).unwrap(),
                    self.prop.u64(self.offset / 16 + 1).unwrap(),
                );
                self.offset += 16;
                result
            }
            _ => panic!("oooops"),
        })
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
        let path = if path.ends_with('/') {
            &path[..path.len() - 1]
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
    use super::PathSplit;

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
