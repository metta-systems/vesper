use core::marker::PhantomData;
use memory::paging::entry::*;
use memory::paging::ENTRY_COUNT;

pub const L0: *mut Table<Level0> = 0xffff_ffff_ffff_f000 as *mut _;

pub struct Table<L: TableLevel> {
    entries: [Entry; ENTRY_COUNT],
    level: PhantomData<L>,
}

impl<L> Table<L>
where
    L: TableLevel,
{
    pub fn zero(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.set_unused();
        }
    }
}

impl<L> Index<usize> for Table<L>
where
    L: TableLevel,
{
    type Output = Entry;

    fn index(&self, index: usize) -> &Entry {
        &self.entries[index]
    }
}

impl<L> IndexMut<usize> for Table<L>
where
    L: TableLevel,
{
    fn index_mut(&mut self, index: usize) -> &mut Entry {
        &mut self.entries[index]
    }
}

pub trait TableLevel {}

pub enum Level0 {}
pub enum Level1 {}
pub enum Level2 {}
pub enum Level3 {}

impl TableLevel for Level0 {}
impl TableLevel for Level1 {}
impl TableLevel for Level2 {}
impl TableLevel for Level3 {}

pub trait HierarchicalLevel: TableLevel {
    type NextLevel: TableLevel;
}

impl HierarchicalLevel for Level0 {
    type NextLevel = Level1;
}
impl HierarchicalLevel for Level1 {
    type NextLevel = Level2;
}
impl HierarchicalLevel for Level2 {
    type NextLevel = Level3;
}
