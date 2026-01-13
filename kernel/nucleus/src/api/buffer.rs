// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Buffer capability with permission tracking in type system
pub struct BufferKey<P: Permission> {
    key: Key<Buffer>,
    size: usize,
    _perm: PhantomData<P>,
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

// ==============================================
// == Kernel space object and syscall handling ==
// ==============================================

/// Buffer kernel object - represents a contiguous memory region
pub struct Buffer {
    phys_base: PhysAddr, // Physical address (fixed at creation)
    size: usize,         // Size in bytes (fixed at creation)
    flags: BufferFlags,  // CACHED, DEVICE, SHARED, etc.

                         // Mapping tracking (for single-address-space)
                         // mappings: SmallVec<Mapping, 4>, // Who has it mapped where
}

struct Mapping {
    domain_id: DomainId,
    virt_addr: VirtAddr,
    permissions: Rights, // May be less than cap rights (derived cap)
}

bitflags::bitflags! {
    pub struct BufferFlags: u32 {
        const CACHED   = 1 << 0;  // Normal cacheable memory
        const DEVICE   = 1 << 1;  // Device memory (uncached, no speculative)
        const SHARED   = 1 << 2;  // Multi-domain sharing expected
        const DMA      = 1 << 3;  // DMA-capable (physically contiguous)
        const EXEC     = 1 << 4;  // Executable (if supported)
    }
}

/// Buffer operations via protected_call
#[repr(u8)]
enum BufferOp {
    Map = 0,   // Map into caller's address space
    Unmap = 1, // Remove mapping
    Query = 2, // Get buffer info (size, flags, mapping status)
}

// =====================
// == Syscall handler ==
// =====================

pub fn invoke(cap: &CapEntry, op: u32, arg0: u64, arg1: u64) -> SyscallResult {
    let buffer = cap.as_buffer()?;
    let caller = current_domain();

    match BufferOp::try_from(op)? {
        BufferOp::Map => {
            // Check MAP right
            if !cap.rights.contains(Rights::MAP) {
                return Err(SyscallError::PermissionDenied);
            }

            // Check not already mapped by this domain
            if buffer.is_mapped_by(caller.id) {
                return Err(SyscallError::AlreadyMapped);
            }

            let hint = if arg0 != 0 {
                Some(VirtAddr::new(arg0))
            } else {
                None
            };
            let flags = MapFlags::from_bits_truncate(arg1 as u32);

            // Allocate virtual address range
            let vaddr = caller.address_space.allocate_range(
                hint,
                buffer.size,
                flags.contains(MapFlags::FIXED),
            )?;

            // Compute page table permissions from cap rights
            let pte_flags = cap.rights.to_pte_flags() | buffer.flags.to_pte_flags();

            // Install page table mappings
            for offset in (0..buffer.size).step_by(PAGE_SIZE) {
                let paddr = buffer.phys_base + offset;
                let vaddr_page = vaddr + offset;

                caller
                    .address_space
                    .map_page(vaddr_page, paddr, pte_flags)?;
            }

            // Record mapping for revocation
            buffer.mappings.push(Mapping {
                domain_id: caller.id,
                virt_addr: vaddr,
                permissions: cap.rights,
            });

            Ok(vaddr.as_u64())
        }

        BufferOp::Unmap => {
            // Find and remove mapping for this domain
            let mapping = buffer
                .mappings
                .iter()
                .position(|m| m.domain_id == caller.id)
                .ok_or(SyscallError::NotMapped)?;

            let mapping = buffer.mappings.remove(mapping);

            // Remove page table entries
            for offset in (0..buffer.size).step_by(PAGE_SIZE) {
                caller
                    .address_space
                    .unmap_page(mapping.virt_addr + offset)?;
            }

            // TLB invalidation (in single-address-space, this is local)
            tlb_invalidate_range(mapping.virt_addr, buffer.size);

            Ok(0)
        }

        BufferOp::Query => {
            let info_ptr = arg0 as *mut BufferInfo;

            // Validate user pointer
            if !caller.address_space.is_valid_user_ptr(info_ptr) {
                return Err(SyscallError::InvalidPointer);
            }

            let mapping = buffer.mappings.iter().find(|m| m.domain_id == caller.id);

            let info = BufferInfo {
                size: buffer.size,
                flags: buffer.flags,
                is_mapped: mapping.is_some(),
                mapped_addr: mapping.map(|m| m.virt_addr),
            };

            unsafe {
                info_ptr.write(info);
            }

            Ok(0)
        }
    }
}
