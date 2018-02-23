// mod arch::aarch64::memory

mod area_frame_allocator;
mod paging;

pub use self::area_frame_allocator::AreaFrameAllocator;

pub type PhysicalAddress = usize;
pub type VirtualAddress = usize;

use self::paging::PAGE_SIZE;

/**
 * Frame is an addressable unit of the physical address space.
 */
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Frame {
    number: usize,
}

impl Frame {
    fn containing_address(address: usize) -> Frame {
        Frame {
            number: address / PAGE_SIZE,
        }
    }

    fn start_address(&self) -> PhysicalAddress {
        self.number * PAGE_SIZE
    }
}

pub trait FrameAllocator {
    fn allocate_frame(&mut self) -> Option<Frame>;
    fn deallocate_frame(&mut self, frame: Frame);
}
