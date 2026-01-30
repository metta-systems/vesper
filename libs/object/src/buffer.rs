// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Buffer capability with permission tracking in type system
pub struct BufferKey<P: Permission> {
    key: Key<Buffer>,
    size: usize,
    _perm: PhantomData<P>,
}

/// Buffer operations via protected_call
#[repr(u8)]
pub enum BufferOp {
    Map = 0,   // Map into caller's address space
    Unmap = 1, // Remove mapping
    Query = 2, // Get buffer info (size, flags, mapping status)
}

pub trait Permission {}
pub struct ReadOnly;
pub struct ReadWrite;
impl Permission for ReadOnly {}
impl Permission for ReadWrite {}

impl<P: Permission> BufferKey<P> {
    /// Map buffer into current domain's address space.
    ///
    /// # Arguments
    /// * `hint` - Optional preferred virtual address (kernel may ignore)
    /// * `flags` - Mapping flags (caching behavior, etc.)
    ///
    /// # Returns
    /// Virtual address where buffer is mapped
    pub fn map(&self, hint: Option<VirtAddr>, flags: MapFlags) -> Result<VirtAddr, Error> {
        let hint_val = hint.map(|a| a.as_u64()).unwrap_or(0);

        let ret = unsafe {
            protected_call2(
                self.key.slot as u64,
                BufferOp::Map as u64,
                hint_val,
                // self.size, ?
                flags.bits() as u64,
            )
        };

        if ret == 0 {
            Err(Error::MapFailed)
        } else {
            Ok(VirtAddr::new(ret))
        }
    }

    /// Unmap buffer from current domain's address space.
    pub fn unmap(&self) -> Result<(), Error> {
        let ret = unsafe { protected_call1(self.key.slot as u64, BufferOp::Unmap as u64, 0) };
        Error::from_code(ret)
    }

    /// Query buffer information (no mapping required)
    pub fn query(&self) -> Result<BufferInfo, Error> {
        let mut info = BufferInfo::default();
        let ret = unsafe {
            protected_call2(
                self.key.slot as u64,
                BufferOp::Query as u64,
                &mut info as *mut _ as u64,
                0,
            )
        };
        if ret == 0 {
            Ok(info)
        } else {
            Err(Error::from_code(ret))
        }
    }
}

// Other ops
impl<P: Permission> BufferKey<P> {
    /// Size of this buffer
    #[inline]
    pub fn size(&self) -> usize {
        self.size
    }
}

impl BufferKey<ReadWrite> {
    /// Derive read-only capability (like &T from &mut T)
    pub fn derive_readonly(&self, dest_slot: u32) -> Result<BufferKey<ReadOnly>, Error> {
        let ret = unsafe {
            protected_call3(
                CAPTBL_SELF,
                KeyTableOp::CopyDerive,
                self.key.slot as u64,
                dest_slot as u64,
                Rights::READ.bits() as u64,
            )
        };
        if ret == 0 {
            Ok(BufferKey {
                key: Cap::new(dest_slot),
                size: self.size,
                _perm: PhantomData,
            })
        } else {
            Err(Error::from_code(ret))
        }
    }

    /// Map and get mutable slice
    pub fn map_slice_mut(&self) -> Result<MappedSliceMut<'_>, Error> {
        let addr = self.map(None, MapFlags::NONE)?;
        Ok(MappedSliceMut {
            cap: self,
            ptr: addr.as_mut_ptr(),
            len: self.size,
        })
    }
}

impl BufferKey<ReadOnly> {
    /// Map and get immutable slice
    pub fn map_slice(&self) -> Result<MappedSlice<'_>, Error> {
        let addr = self.map(None, MapFlags::NONE)?;
        Ok(MappedSlice {
            cap: self,
            ptr: addr.as_ptr(),
            len: self.size,
        })
    }
}

#[derive(Default)]
pub struct BufferInfo {
    pub size: usize,
    pub flags: BufferFlags,
    pub is_mapped: bool,
    pub mapped_addr: Option<VirtAddr>,
}

bitflags::bitflags! {
    pub struct MapFlags: u32 {
        const NONE       = 0;
        const FIXED      = 1 << 0;  // Fail if hint can't be used
        const POPULATE   = 1 << 1;  // Pre-fault all pages
        const UNCACHED   = 1 << 2;  // Override to uncached
    }
}

/// RAII guard that unmaps on drop
pub struct MappedSlice<'a> {
    cap: &'a BufferKey<ReadOnly>,
    ptr: *const u8,
    len: usize,
}

impl<'a> MappedSlice<'a> {
    pub fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl Drop for MappedSlice<'_> {
    fn drop(&mut self) {
        let _ = self.cap.unmap();
    }
}

pub struct MappedSliceMut<'a> {
    cap: &'a BufferKey<ReadWrite>,
    ptr: *mut u8,
    len: usize,
}

impl<'a> MappedSliceMut<'a> {
    pub fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for MappedSliceMut<'_> {
    fn drop(&mut self) {
        let _ = self.cap.unmap();
    }
}
