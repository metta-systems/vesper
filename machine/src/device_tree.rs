use shrinkwraprs::Shrinkwrap;
use fdt_rs::base::{DevTree, DevTreeProp};

#[derive(Shrinkwrap)]
pub struct DeadTree<'a>(DevTree<'a>);

// This is based on lib.rs/dtb implementation.
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

impl<'a> DeadTree<'a> {
    #[allow(unused)]
    pub fn new(reader: DevTree<'a>) -> Self {
        Self(reader)
    }

    pub fn find_prop_by_path(&self, path: &str) -> Result<bool, ()> {
        // iterate path separated by / starting from root "/" and find props one by one
        let path = PathSplit::new(path);
        while !path.is_finished() {
            self.0.nodes();
            // "/#address-cells" - is a prop inside root node
        }
        Ok(false)
    }

    // pub fn try_struct_u32_value<'s, P: Into<&'s str>>(&self, path: P) -> Result<u32, dtb::Error> {
    //     let mut buf = [0u8; 4];
    //     Ok(self
    //         .0
    //         .struct_items()
    //         .path_struct_items(path.into())
    //         .next()
    //         .ok_or(dtb::Error::BadPropertyName)?
    //         .0
    //         .value_u32_list(&mut buf)?[0])
    // }
    //
    // pub fn try_struct_str_value<'s, P: Into<&'s str>>(&self, path: P) -> Result<&str, dtb::Error> {
    //     self.0
    //         .struct_items()
    //         .path_struct_items(path.into())
    //         .next()
    //         .ok_or(dtb::Error::BadPropertyName)?
    //         .0
    //         .value_str()
    // }
}
