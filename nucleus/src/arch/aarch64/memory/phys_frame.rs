// Verbatim from https://github.com/rust-osdev/x86_64/blob/aa9ae54657beb87c2a491f2ab2140b2332afa6ba/src/structures/paging/frame.rs
// Abstractions for default-sized and huge physical memory frames.

use {
    crate::memory::{
        page_size::{PageSize, Size4KiB},
        PhysAddr,
    },
    core::{
        fmt,
        marker::PhantomData,
        ops::{Add, AddAssign, Sub, SubAssign},
    },
};

/// A physical memory frame.
/// Frame is an addressable unit of the physical address space.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(C)]
pub struct PhysFrame<S: PageSize = Size4KiB> {
    start_address: PhysAddr,
    size: PhantomData<S>,
}

impl<S: PageSize> From<u64> for PhysFrame<S> {
    fn from(address: u64) -> PhysFrame<S> {
        PhysFrame::containing_address(PhysAddr::new(address))
    }
}

impl<S: PageSize> From<PhysFrame<S>> for u64 {
    fn from(frame: PhysFrame<S>) -> u64 {
        frame.start_address.as_u64()
    }
}

impl<S: PageSize> PhysFrame<S> {
    /// Returns the frame that starts at the given virtual address.
    ///
    /// Returns an error if the address is not correctly aligned (i.e. is not a valid frame start).
    pub fn from_start_address(address: PhysAddr) -> Result<Self, ()> {
        if !address.is_aligned(S::SIZE) {
            return Err(());
        }
        Ok(PhysFrame::containing_address(address))
    }

    /// Returns the frame that contains the given physical address.
    pub fn containing_address(address: PhysAddr) -> Self {
        PhysFrame {
            start_address: address.aligned_down(S::SIZE),
            size: PhantomData,
        }
    }

    /// Returns the start address of the frame.
    pub fn start_address(&self) -> PhysAddr {
        self.start_address
    }

    /// Returns the size the frame (4KB, 2MB or 1GB).
    pub fn size(&self) -> usize {
        S::SIZE
    }

    /// Returns a range of frames, exclusive `end`.
    pub fn range(start: PhysFrame<S>, end: PhysFrame<S>) -> PhysFrameRange<S> {
        PhysFrameRange { start, end }
    }

    /// Returns a range of frames, inclusive `end`.
    pub fn range_inclusive(start: PhysFrame<S>, end: PhysFrame<S>) -> PhysFrameRangeInclusive<S> {
        PhysFrameRangeInclusive { start, end }
    }
}

impl<S: PageSize> fmt::Debug for PhysFrame<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!(
            "PhysFrame[{}]({:#x})",
            S::SIZE_AS_DEBUG_STR,
            self.start_address().as_u64()
        ))
    }
}

impl<S: PageSize> Add<u64> for PhysFrame<S> {
    type Output = Self;
    /// Adds `rhs` same-sized frames to the current address.
    fn add(self, rhs: u64) -> Self::Output {
        PhysFrame::containing_address(self.start_address() + rhs * S::SIZE as u64)
    }
}

impl<S: PageSize> AddAssign<u64> for PhysFrame<S> {
    fn add_assign(&mut self, rhs: u64) {
        *self = self.clone() + rhs;
    }
}

impl<S: PageSize> Sub<u64> for PhysFrame<S> {
    type Output = Self;
    /// Subtracts `rhs` same-sized frames from the current address.
    // @todo should I sub pages or just bytes here?
    fn sub(self, rhs: u64) -> Self::Output {
        PhysFrame::containing_address(self.start_address() - rhs * S::SIZE as u64)
    }
}

impl<S: PageSize> SubAssign<u64> for PhysFrame<S> {
    fn sub_assign(&mut self, rhs: u64) {
        *self = self.clone() - rhs;
    }
}

impl<S: PageSize> Sub<PhysFrame<S>> for PhysFrame<S> {
    type Output = usize;
    /// Return number of frames between start and end addresses.
    fn sub(self, rhs: PhysFrame<S>) -> Self::Output {
        (self.start_address - rhs.start_address) as usize / S::SIZE
    }
}

/// A range of physical memory frames, exclusive the upper bound.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct PhysFrameRange<S: PageSize = Size4KiB> {
    /// The start of the range, inclusive.
    pub start: PhysFrame<S>,
    /// The end of the range, exclusive.
    pub end: PhysFrame<S>,
}

impl<S: PageSize> PhysFrameRange<S> {
    /// Returns whether the range contains no frames.
    pub fn is_empty(&self) -> bool {
        !(self.start < self.end)
    }
}

impl<S: PageSize> Iterator for PhysFrameRange<S> {
    type Item = PhysFrame<S>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.start < self.end {
            let frame = self.start.clone();
            self.start += 1;
            Some(frame)
        } else {
            None
        }
    }
}

impl<S: PageSize> fmt::Debug for PhysFrameRange<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("PhysFrameRange")
            .field("start", &self.start)
            .field("end", &self.end)
            .finish()
    }
}

/// An range of physical memory frames, inclusive the upper bound.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct PhysFrameRangeInclusive<S: PageSize = Size4KiB> {
    /// The start of the range, inclusive.
    pub start: PhysFrame<S>,
    /// The start of the range, exclusive.
    pub end: PhysFrame<S>,
}

impl<S: PageSize> PhysFrameRangeInclusive<S> {
    /// Returns whether the range contains no frames.
    pub fn is_empty(&self) -> bool {
        !(self.start <= self.end)
    }
}

impl<S: PageSize> Iterator for PhysFrameRangeInclusive<S> {
    type Item = PhysFrame<S>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.start <= self.end {
            let frame = self.start.clone();
            self.start += 1;
            Some(frame)
        } else {
            None
        }
    }
}

impl<S: PageSize> fmt::Debug for PhysFrameRangeInclusive<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("PhysFrameRangeInclusive")
            .field("start", &self.start)
            .field("end", &self.end)
            .finish()
    }
}
