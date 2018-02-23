use super::super::Frame;

pub struct Entry(u64);

bitflags! {
    pub struct EntryFlags: u64 {
        const VALID          = 1 <<  0;
        const TABLE          = 1 <<  1; // If set, a table entry, otherwise block entry
        // ATTR_INDEX_MASK = 7 << 2;
        const NON_SECURE     = 1 <<  5; // block/page descriptor lower attributes
        const ACCESS         = 1 << 10; // block/page descriptor lower attributes
        const NOT_GLOBAL     = 1 << 11; // nG, block/page descriptor lower attributes
        const DIRTY          = 1 << 51; // block/page descriptor upper attributes
        const CONTIGUOUS     = 1 << 52; // block/page descriptor upper attributes
        const EL1_EXEC_NEVER = 1 << 53; // block/page descriptor upper attributes
        const EXEC_NEVER     = 1 << 54; // block/page descriptor upper attributes
        const PXN_TABLE      = 1 << 59; // table descriptor, next level table attributes
        const XN_TABLE       = 1 << 60; // table descriptor, next level table attributes
        const AP_TABLE       = 1 << 61; // table descriptor, next level table attributes, 2 bits
        const NS_TABLE       = 1 << 63; // table descriptor, next level table attributes
    }
}

impl Entry {
    pub fn is_unused(&self) -> bool {
        self.0 == 0
    }

    pub fn set_unused(&mut self) {
        self.0 = 0;
    }

    pub fn flags(&self) -> EntryFlags {
        EntryFlags::from_bits_truncate(self.0)
    }

    pub fn pointed_frame(&self) -> Option<Frame> {
        if self.flags().contains(EntryFlags::VALID) {
            Some(Frame::containing_address(
                self.0 as usize & 0x0000_ffff_ffff_f000,
            ))
        } else {
            None
        }
    }

    pub fn set(&mut self, frame: Frame, flags: EntryFlags) {
        assert!(frame.start_address() & !0x0000_ffff_ffff_f000 == 0);
        self.0 = (frame.start_address() as u64) | flags.bits();
    }
}
