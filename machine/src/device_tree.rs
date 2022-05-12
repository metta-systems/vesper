use shrinkwraprs::Shrinkwrap;

#[derive(Shrinkwrap)]
pub struct DeadTree<'a>(dtb::Reader<'a>);

impl<'a> DeadTree<'a> {
    pub fn new(reader: dtb::Reader<'a>) -> Self {
        Self(reader)
    }

    pub fn try_struct_u32_value<'s, P: Into<&'s str>>(&self, path: P) -> Result<u32, dtb::Error> {
        let mut buf = [0u8; 4];
        Ok(self
            .0
            .struct_items()
            .path_struct_items(path.into())
            .next()
            .ok_or(dtb::Error::BadPropertyName)?
            .0
            .value_u32_list(&mut buf)?[0])
    }

    pub fn try_struct_str_value<'s, P: Into<&'s str>>(&self, path: P) -> Result<&str, dtb::Error> {
        self.0
            .struct_items()
            .path_struct_items(path.into())
            .next()
            .ok_or(dtb::Error::BadPropertyName)?
            .0
            .value_str()
    }
}
