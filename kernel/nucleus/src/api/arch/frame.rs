use {
    crate::{
        api::key_entry::KeyEntry,
        objects::{ArchObjects, nucleus::Nucleus},
    },
    libobject::{CapError, Rights, arch::frame::FrameOp},
};

/// Invoke a frame capability operation.
///
/// The frame data is stored inline in the KeyEntry as a RegionPayload.
/// Each frame cap tracks its own single mapping (seL4-style).
/// To map the same physical frame at two addresses, duplicate the cap first.
pub fn invoke<A: ArchObjects>(
    entry: &mut KeyEntry,
    op: u32,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let op = FrameOp::try_from(op as u8).map_err(|_| CapError::InvalidOperation)?;
    let rights = entry.rights();

    match op {
        FrameOp::Map => {
            // args[0] = vspace_slot
            // args[1] = virt_addr
            // args[2] = rights (R/W/X bits)
            // args[3] = attrs (cacheability, etc.)

            if !rights.contains(Rights::READ) {
                return Err(CapError::InsufficientRights);
            }

            let frame = entry.as_frame_mut()?;
            if frame.is_mapped() {
                return Err(CapError::AlreadyMapped);
            }

            let vaddr = args[1];
            // ... perform the mapping via arch-specific page table code ...
            frame.set_mapped(vaddr);

            Ok((0, 0))
        }

        FrameOp::Unmap => {
            let frame = entry.as_frame_mut()?;
            if !frame.is_mapped() {
                return Err(CapError::NotMapped);
            }
            // ... perform the unmapping using frame.mapped_vaddr() ...
            frame.clear_mapped();
            Ok((0, 0))
        }

        FrameOp::GetAddress => {
            if !rights.contains(Rights::GRANT) {
                return Err(CapError::InsufficientRights);
            }
            let frame = entry.as_frame()?;
            Ok((frame.paddr, frame.size() as u64))
        }

        FrameOp::Remap => {
            // Change attributes on existing mapping
            todo!("frame remap");
        }
    }
}
