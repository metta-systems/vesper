/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

use crate::{arch, arch::memory::VirtAddr};

// trait Thread {
//     // Configuration
//     // Effectively, SetSpace followed by SetIPCBuffer.
//     fn configure(fault_endpoint: Cap, cap_space_root: Cap, cap_space_root_data: CapNodeConfig, virt_space_root: Cap, virt_space_root_data: (), ipc_buffer_frame: Cap, ipc_buffer_offset: usize) -> Result<()>;
//     fn set_space(fault_endpoint: Cap, cap_space_root: Cap, cap_space_root_data: CapNodeConfig, virt_space_root: Cap, virt_space_root_data: ()) -> Result<()>;
//     fn set_ipc_buffer(ipc_buffer_frame: CapNode, ipc_buffer_offset: usize) -> Result<()>;
//     // Debugging tools
//     fn configure_single_stepping(bp_num: u16, num_insns): Result<SingleStepping>;
//     fn get_breakpoint(bp_num: u16) -> Result<BreakpointInfo>;
//     fn set_breakpoint(bp_num: u16, bp: BreakpointInfo) -> Result<()>;
//     fn unset_breakpoint(bp_num: u16) -> Result<()>;
//     // Scheduling
//     fn suspend() -> Result<()>;
//     fn resume() -> Result<()>;
//     fn set_priority(authority: TCB/*Cap*/, priority: u32) -> Result<()>;
//     fn set_mc_priority(authority: TCB/*Cap*/, mcp: u32) -> Result<()>;
//     fn set_sched_params(authority: TCB/*Cap*/, mcp: u32, priority: u32) -> Result<()>;
//     fn set_affinity(affinity: u64) -> Result<()>;
//     // TCB configuration
//     fn copy_registers(source: TCB/*Cap*/, suspend_source: bool, resume_target: bool, transfer_frame_regs: bool, transfer_integer_regs: bool, arch_flags: u8) -> Result<()>;
//     fn read_registers(suspend_source: bool, arch_flags: u8, num_regs: u16, register_context: &mut ArchRegisterContext) -> Result<()>;
//     fn write_registers(resume_target: bool, arch_flags: u8, num_regs: u16, register_context: &ArchRegisterContext) -> Result<()>;
//     // Notifications
//     fn bind_notification(notification: CapNode) -> Result<()>;
//     fn unbind_notification() -> Result<()>;
//
//     // Arch-specific
//     // fn set_tls_base(tls_base: usize) -> Result<()>;
//     // virtualized - x86-specific
//     // fn set_ept_root(eptpml: X86::EPTPML4) -> Result<()>;
// }
//
// // @todo <<SchedContext>>
//
// // struct Thread {}
// struct TCB {
//     capability: u128, // should actually be a CapPath here - this is the argument to
//     // Thread.read_registers(cap, ... call for example.
// }
//
// impl Thread for TCB {
//     // ...
// }

// impl super::KernelObject for Thread {}
impl super::KernelObject for TCB {
    const SIZE_BITS: usize = 12;
}

// -- from actual code parts in api.rs

/* TCB: size 64 bytes + sizeof(arch_tcb_t) (aligned to nearest power of 2) */
struct TCB {
    arch_specific: arch::objects::TCB,
    state: ThreadState, // 12 bytes?
    /* Notification that this TCB is bound to. If this is set, when this TCB waits on
     * any sync endpoint, it may receive a signal from a Notification object.
     * 4 bytes*/
    // notification_t *tcbBoundNotification;
    fault: Fault,                // 8 bytes?
    lookup_failure: LookupFault, // 8 bytes
    /* Domain, 1 byte (packed to 4) */
    domain: Domain,
    /*  maximum controlled priorioty, 1 byte (packed to 4) */
    mcp: Priority,
    /* Priority, 1 byte (packed to 4) */
    priority: Priority,
    /* Timeslice remaining, 4 bytes */
    time_slice: u32,
    /* Capability pointer to thread fault handler, 8 bytes */
    fault_handler: CapPath,
    /* userland virtual address of thread IPC buffer, 8 bytes */
    ipc_buffer: VirtAddr,
    /* Previous and next pointers for scheduler queues , 8+8 bytes */
    sched_next: *mut TCB,
    sched_prev: *mut TCB,
    /* Previous and next pointers for endpoint and notification queues, 8+8 bytes */
    ep_next: *mut TCB,
    ep_prev: *mut TCB,
    /* Use any remaining space for a thread name */
    name: &str,
    // name_storage: [u8],// add SIZE_BITS calculations for length of storage in here somewhere
}

pub(crate) enum ThreadState {
    Inactive,
    Restart,
}

impl TCB {
    fn get_register(&self, register_index: usize) {
        self.arch_tcb.register_context.registers[register_index]
    }
    fn lookup_cap_and_slot() -> Result<()> {}
    fn get_restart_pc() {}
    fn lookup_ipc_buffer(some: bool) {}
    fn lookup_extra_caps() -> Result<()> {}
    fn get_state() -> ThreadState {}
    fn set_state(state: ThreadState) {}

    fn get_caller_slot() -> Slot {}
    fn send_fault_ipc(&self) {}

    fn replyFromKernel_success_empty() {}
    fn replyFromKernel_error() {}
    //     // Configuration
    //     // Effectively, SetSpace followed by SetIPCBuffer.
    //     fn configure(fault_endpoint: Cap, cap_space_root: Cap, cap_space_root_data: CapNodeConfig, virt_space_root: Cap, virt_space_root_data: (), ipc_buffer_frame: Cap, ipc_buffer_offset: usize) -> Result<()>;
    //     fn set_space(fault_endpoint: Cap, cap_space_root: Cap, cap_space_root_data: CapNodeConfig, virt_space_root: Cap, virt_space_root_data: ()) -> Result<()>;
    //     fn set_ipc_buffer(ipc_buffer_frame: CapNode, ipc_buffer_offset: usize) -> Result<()>;
    //     // Debugging tools
    //     fn configure_single_stepping(bp_num: u16, num_insns): Result<SingleStepping>;
    //     fn get_breakpoint(bp_num: u16) -> Result<BreakpointInfo>;
    //     fn set_breakpoint(bp_num: u16, bp: BreakpointInfo) -> Result<()>;
    //     fn unset_breakpoint(bp_num: u16) -> Result<()>;
    //     // Scheduling
    //     fn suspend() -> Result<()>;
    //     fn resume() -> Result<()>;
    //     fn set_priority(authority: TCB/*Cap*/, priority: u32) -> Result<()>;
    //     fn set_mc_priority(authority: TCB/*Cap*/, mcp: u32) -> Result<()>;
    //     fn set_sched_params(authority: TCB/*Cap*/, mcp: u32, priority: u32) -> Result<()>;
    //     fn set_affinity(affinity: u64) -> Result<()>;
    //     // TCB configuration
    //     fn copy_registers(source: TCB/*Cap*/, suspend_source: bool, resume_target: bool, transfer_frame_regs: bool, transfer_integer_regs: bool, arch_flags: u8) -> Result<()>;
    //     fn read_registers(suspend_source: bool, arch_flags: u8, num_regs: u16, register_context: &mut ArchRegisterContext) -> Result<()>;
    //     fn write_registers(resume_target: bool, arch_flags: u8, num_regs: u16, register_context: &ArchRegisterContext) -> Result<()>;
    //     // Notifications
    //     fn bind_notification(notification: CapNode) -> Result<()>;
    //     fn unbind_notification() -> Result<()>;
}
