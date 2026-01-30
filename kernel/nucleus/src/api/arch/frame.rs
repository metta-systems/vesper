use libobject::arch::frame::FrameOp;

// pub trait FrameInvoke { fn invoke() }

pub fn invoke<A: ArchObjects>(
    frame: &mut A::Frame,
    rights: Rights,
    op: u32,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let op = FrameOp::try_from(op as u8).map_err(|_| CapError::InvalidOperation)?;

    match op {
        FrameOp::Map => {
            // args[0] = vspace_slot
            // args[1] = virt_addr
            // args[2] = rights (R/W/X bits)
            // args[3] = attrs (cacheability, etc.)

            if !rights.contains(Rights::READ) {
                return Err(CapError::InsufficientRights);
            }

            let vspace_slot = KeySlot(args[0] as u16);
            let virt_addr = VirtAddr::new(args[1]);
            let map_rights = MapRights::from_bits(args[2] as u8);
            let attrs = MemAttrs::from_bits(args[3] as u8);

            // Get the VSpace from the slot
            let domain = kernel.current_domain()?;
            let vspace_entry = domain.keytable.lookup(vspace_slot)?;
            let vspace = vspace_entry.as_object::<AArch64VSpace>()?;

            // Perform the mapping
            // aarch64_map_frame(frame, vspace, virt_addr, map_rights, attrs, kernel)?;

            Ok((0, 0))
        }

        FrameOp::Unmap => {
            if frame.map_count == 0 {
                return Err(CapError::NotMapped);
            }
            // ... unmap logic
            todo!("frame unmap");
            Ok((0, 0))
        }

        FrameOp::GetAddress => {
            // Requires Grant right to expose physical address
            if !rights.contains(Rights::GRANT) {
                return Err(CapError::InsufficientRights);
            }
            Ok((frame.phys_addr.as_u64(), frame.size.size() as u64))
        }

        FrameOp::Remap => {
            // Change attributes on existing mapping
            todo!("frame remap");
        }
    }
}
