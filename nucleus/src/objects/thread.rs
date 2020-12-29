/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

trait Thread {
    // Configuration
    // Effectively, SetSpace followed by SetIPCBuffer.
    fn configure(fault_endpoint: Cap, cap_space_root: Cap, cap_space_root_data: CapNodeConfig, virt_space_root: Cap, virt_space_root_data: (), ipc_buffer_frame: Cap, ipc_buffer_offset: usize) -> Result<()>;
    fn set_space(fault_endpoint: Cap, cap_space_root: Cap, cap_space_root_data: CapNodeConfig, virt_space_root: Cap, virt_space_root_data: ()) -> Result<()>;
    fn set_ipc_buffer(ipc_buffer_frame: CapNode, ipc_buffer_offset: usize) -> Result<()>;
    // Debugging tools
    fn configure_single_stepping(bp_num: u16, num_insns): Result<SingleStepping>;
    fn get_breakpoint(bp_num: u16) -> Result<BreakpointInfo>;
    fn set_breakpoint(bp_num: u16, bp: BreakpointInfo) -> Result<()>;
    fn unset_breakpoint(bp_num: u16) -> Result<()>;
    // Scheduling
    fn suspend() -> Result<()>;
    fn resume() -> Result<()>;
    fn set_priority(authority: TCB/*Cap*/, priority: u32) -> Result<()>;
    fn set_mc_priority(authority: TCB/*Cap*/, mcp: u32) -> Result<()>;
    fn set_sched_params(authority: TCB/*Cap*/, mcp: u32, priority: u32) -> Result<()>;
    fn set_affinity(affinity: u64) -> Result<()>;
    // TCB configuration
    fn copy_registers(source: TCB/*Cap*/, suspend_source: bool, resume_target: bool, transfer_frame_regs: bool, transfer_integer_regs: bool, arch_flags: u8) -> Result<()>;
    fn read_registers(suspend_source: bool, arch_flags: u8, num_regs: u16, register_context: &mut ArchRegisterContext) -> Result<()>;
    fn write_registers(resume_target: bool, arch_flags: u8, num_regs: u16, register_context: &ArchRegisterContext) -> Result<()>;
    // Notifications
    fn bind_notification(notification: CapNode) -> Result<()>;
    fn unbind_notification() -> Result<()>;

    // Arch-specific
    // fn set_tls_base(tls_base: usize) -> Result<()>;
    // virtualized - x86-specific
    // fn set_ept_root(eptpml: X86::EPTPML4) -> Result<()>;
}

// @todo <<SchedContext>>

// struct Thread {}
struct TCB {
    capability: u128, // should actually be a CapPath here - this is the argument to
    // Thread.read_registers(cap, ... call for example.
}

impl Thread for TCB {
    // ...
}

// impl super::KernelObject for Thread {}
impl super::KernelObject for TCB {
    const SIZE_BITS: usize = 12;
}
