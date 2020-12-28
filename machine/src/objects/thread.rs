/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

struct Thread {}

impl super::KernelObject for Thread {}


trait Thread {
    // Effectively, SetSpace followed by SetIPCBuffer.
    fn configure(fault_endpoint: CapNode, cap_space_root: CapNode, cap_space_root_data: CapNodeConfig, virt_space_root: CapNode, virt_space_root_data: ??, ipc_buffer_frame: CapNode, ipc_buffer_offset: usize) -> Result<()>;
    fn set_space(fault_endpoint: CapNode, cap_space_root: CapNode, cap_space_root_data: CapNodeConfig, virt_space_root: CapNode, virt_space_root_data: ??) -> Result<()>;
    fn configure_single_stepping(bp_num: u16, num_insns): Result<SingleStepping>;
    fn get_breakpoint(bp_num: u16) -> Result<BreakpointInfo>;
    fn set_breakpoint(bp_num: u16, bp: BreakpointInfo) -> Result<()>;
    fn unset_breakpoint(bp_num: u16) -> Result<()>;
    fn suspend() -> Result<()>;
    fn resume() -> Result<()>;
    fn copy_registers(source: TCB/*Cap*/, suspend_source: bool, resume_target: bool, transfer_frame_regs: bool, transfer_integer_regs: bool, arch_flags: u8) -> Result<()>;
    fn read_registers(suspend_source: bool, arch_flags: u8, num_regs: u16, register_context: &mut ArchRegisterContext) -> Result<()>;
    fn write_registers(resume_target: bool, arch_flags: u8, num_regs: u16, register_context: &ArchRegisterContext) -> Result<()>;
    fn bind_notification(notification: CapNode) -> Result<()>;
    fn unbind_notification() -> Result<()>;
    fn set_priority(authority: TCB/*Cap*/, priority: u32) -> Result<()>;
    fn set_mc_priority(authority: TCB/*Cap*/, mcp: u32) -> Result<()>;
    fn set_sched_params(authority: TCB/*Cap*/, mcp: u32, priority: u32) -> Result<()>;
    fn set_affinity(affinity: u64) -> Result<()>;
    fn set_ipc_buffer(ipc_buffer_frame: CapNode, ipc_buffer_offset: usize) -> Result<()>;
    // Arch-specific
    fn set_tls_base(tls_base: usize) -> Result<()>;
    // virtualized - x86-specific
    fn set_ept_root(eptpml: X86::EPTPML4) -> Result<()>;
}

// @todo <<SchedContext>>

struct TCB {
    capability: u128, // should actually be a CapPath here - this is the argument to
    // Thread.read_registers(cap, ... call for example.
}

impl Thread for TCB {
    fn configure(fault_endpoint: _, cap_space_root: _, cap_space_root_data: _, virt_space_root: _, ipc_buffer_frame: _, ipc_buffer_offset: usize) -> _ {
        unimplemented!()
    }

    fn set_space(fault_endpoint: _, cap_space_root: _, cap_space_root_data: _, virt_space_root: _) -> _ {
        unimplemented!()
    }

    fn configure_single_stepping(bp_num: u16, _: _) {
        unimplemented!()
    }

    fn get_breakpoint(bp_num: u16) -> _ {
        unimplemented!()
    }

    fn set_breakpoint(bp_num: u16, bp: _) -> _ {
        unimplemented!()
    }

    fn unset_breakpoint(bp_num: u16) -> _ {
        unimplemented!()
    }

    fn suspend() -> _ {
        unimplemented!()
    }

    fn resume() -> _ {
        unimplemented!()
    }

    fn copy_registers(source: TCB, suspend_source: bool, resume_target: bool, transfer_frame_regs: bool, transfer_integer_regs: bool, arch_flags: u8) -> _ {
        unimplemented!()
    }

    fn read_registers(suspend_source: bool, arch_flags: u8, num_regs: u16, register_context: &mut _) -> _ {
        unimplemented!()
    }

    fn write_registers(resume_target: bool, arch_flags: u8, num_regs: u16, register_context: &_) -> _ {
        unimplemented!()
    }

    fn bind_notification(notification: _) -> _ {
        unimplemented!()
    }

    fn unbind_notification() -> _ {
        unimplemented!()
    }

    fn set_priority(authority: TCB, priority: u32) -> _ {
        unimplemented!()
    }

    fn set_mc_priority(authority: TCB, mcp: u32) -> _ {
        unimplemented!()
    }

    fn set_sched_params(authority: TCB, mcp: u32, priority: u32) -> _ {
        unimplemented!()
    }

    fn set_affinity(affinity: u64) -> _ {
        unimplemented!()
    }

    fn set_ipc_buffer(ipc_buffer_frame: _, ipc_buffer_offset: usize) -> _ {
        unimplemented!()
    }

    fn set_tls_base(tls_base: usize) -> _ {
        unimplemented!()
    }

    fn set_ept_root(eptpml: _) -> _ {
        unimplemented!()
    }
}

impl KernelObject for TCB {
    const SIZE_BITS: usize = 12;
}
