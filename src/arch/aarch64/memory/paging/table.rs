use super::super::FrameAllocator;
use super::ENTRY_COUNT;
use arch::aarch64::memory::paging::entry::*;
use core::marker::PhantomData;
use core::ops::{Index, IndexMut};

pub const L0: *mut Table<Level0> = 0xffff_ffff_ffff_f000 as *mut _; // L0 0o177777_777_777_777_777_0000
                                                                    // L1 0o177777_777_777_777_XXX_0000, XXX is the L0 index
                                                                    // L2 0o177777_777_777_XXX_YYY_0000, YYY is the L1 index
                                                                    // L3 0o177777_777_XXX_YYY_ZZZ_0000, ZZZ is the L2 index

// L1 = (L0 << 9) | (XXX << 12)
// L2 = (L1 << 9) | (YYY << 12)
// L3 = (L2 << 9) | (ZZZ << 12)

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

impl<L> Table<L>
where
    L: HierarchicalLevel,
{
    fn next_table_address(&self, index: usize) -> Option<usize> {
        let entry_flags = self[index].flags();
        if entry_flags.contains(EntryFlags::VALID | EntryFlags::TABLE) {
            let table_address = self as *const _ as usize;
            Some((table_address << 9) | (index << 12))
        } else {
            None
        }
    }

    pub fn next_table(&self, index: usize) -> Option<&Table<L::NextLevel>> {
        self.next_table_address(index)
            .map(|address| unsafe { &*(address as *const _) })
    }

    pub fn next_table_mut(&mut self, index: usize) -> Option<&mut Table<L::NextLevel>> {
        self.next_table_address(index)
            .map(|address| unsafe { &mut *(address as *mut _) })
    }

    pub fn next_table_create<A>(
        &mut self,
        index: usize,
        allocator: &mut A,
    ) -> &mut Table<L::NextLevel>
    where
        A: FrameAllocator,
    {
        if self.next_table(index).is_none() {
            assert!(
                self.entries[index].flags().contains(EntryFlags::TABLE),
                "mapping code does not support huge pages"
            );
            let frame = allocator.allocate_frame().expect("no frames available");
            self.entries[index].set(frame, EntryFlags::VALID /*| WRITABLE*/);
            self.next_table_mut(index).unwrap().zero();
        }
        self.next_table_mut(index).unwrap()
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
