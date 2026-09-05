#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rights(pub u8);

impl Rights {
    pub const READ: u8 = 0x1;
    pub const WRITE: u8 = 0x2;
    pub const MAP: u8 = 0x2;
    pub const SEND: u8 = 0x2;
    pub const RECV: u8 = 0x1;
    pub const CALL: u8 = 0x4;
    pub const GRANT: u8 = 0x8;

    pub const fn empty() -> Rights {
        Rights(0)
    }
    pub const fn all() -> Rights {
        Rights(0xF)
    }
    pub fn bits(&self) -> u8 {
        self.0
    }
}
