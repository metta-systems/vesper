// ═══════════════════════════════════════════════════════════════════
// ARCHITECTURE-SPECIFIC API HANDLERS
// ═══════════════════════════════════════════════════════════════════

pub mod api {
    pub mod arch {
        use super::*;

        pub mod frame {
            #[repr(u8)]
            pub enum FrameOp {
                /// Map frame into a VSpace at given virtual address
                Map = 0,
                /// Unmap frame from VSpace
                Unmap = 1,
                /// Get physical address (requires special rights)
                GetAddress = 2,
                /// Remap with different attributes
                Remap = 3,
            }

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
                        // args[2] = attrs (R/W/X)
                        if !rights.contains(Rights::READ) {
                            return Err(CapError::InsufficientRights);
                        }
                        // Implementation depends on A::Frame
                        todo!("frame map")
                    }
                    FrameOp::Unmap => {
                        todo!("frame unmap")
                    }
                    FrameOp::GetAddress => {
                        if !rights.contains(Rights::GRANT) {
                            return Err(CapError::InsufficientRights);
                        }
                        // Return physical address
                        todo!("frame get_address")
                    }
                    FrameOp::Remap => {
                        todo!("frame remap")
                    }
                }
            }
        }

        pub mod vspace {
            #[repr(u8)]
            pub enum VSpaceOp {
                /// Assign root page table
                SetRoot = 0,
                /// Assign ASID
                AssignASID = 1,
                /// Activate (switch to this address space)
                Activate = 2,
                /// Get current ASID
                GetASID = 3,
            }

            pub fn invoke<A: ArchObjects>(
                vspace: &mut A::VSpace,
                rights: Rights,
                op: u32,
                args: &[u64; 6],
                kernel: &mut Kernel<A>,
            ) -> Result<(u64, u64), CapError> {
                let op = VSpaceOp::try_from(op as u8).map_err(|_| CapError::InvalidOperation)?;

                match op {
                    VSpaceOp::SetRoot => {
                        // args[0] = page_table_slot
                        todo!("vspace set_root")
                    }
                    VSpaceOp::AssignASID => {
                        // args[0] = asid_pool_slot
                        todo!("vspace assign_asid")
                    }
                    VSpaceOp::Activate => {
                        todo!("vspace activate")
                    }
                    VSpaceOp::GetASID => {
                        todo!("vspace get_asid")
                    }
                }
            }
        }

        pub mod page_table {
            #[repr(u8)]
            pub enum PageTableOp {
                /// Map a page table into parent table
                Map = 0,
                /// Unmap from parent
                Unmap = 1,
            }

            pub fn invoke<A: ArchObjects>(
                pt: &mut A::PageTable,
                rights: Rights,
                op: u32,
                args: &[u64; 6],
                kernel: &mut Kernel<A>,
            ) -> Result<(u64, u64), CapError> {
                todo!("page_table invoke")
            }
        }

        pub mod asid_pool {
            #[repr(u8)]
            pub enum ASIDPoolOp {
                /// Allocate an ASID from this pool
                Allocate = 0,
            }

            pub fn invoke<A: ArchObjects>(
                pool: &mut A::ASIDPool,
                rights: Rights,
                op: u32,
                args: &[u64; 6],
                kernel: &mut Kernel<A>,
            ) -> Result<(u64, u64), CapError> {
                todo!("asid_pool invoke")
            }
        }

        pub mod asid {
            pub fn invoke<A: ArchObjects>(
                asid: &mut A::ASID,
                rights: Rights,
                op: u32,
                args: &[u64; 6],
            ) -> Result<(u64, u64), CapError> {
                // ASIDs mostly just exist; operations are minimal
                todo!("asid invoke")
            }
        }
    }
}
