/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */
// The basic services Vesper provides are as follows:
//
// * _Threads_ are an abstraction of CPU execution that supports running software;
// * _Address spaces_ are virtual memory spaces that each contain an application.
//                    Applications are limited to accessing memory in their address space;
// * _Inter-process communication (IPC)_ via endpoints allows threads to communicate using
//                                       message passing;
// * _Events_ provide a non-blocking signalling mechanism similar to counting semaphores;
// * _Device primitives_ allow device drivers to be implemented as unprivileged applications.
//                       The kernel exports hardware device interrupts via IPC messages; and
// * _Capability spaces_ store capabilities (i.e., access rights) to kernel services along with
//                       their book-keeping information.

//================
// Kernel objects
//================

register_bitfields! {
    u128,
    Endpoint [
        QueueHead OFFSET(0) NUMBITS(64) [],
        QueueTail OFFSET(80) NUMBITS(46) [],
        State OFFSET(126) NUMBITS(2) [
            Idle = 00b,
            Send = 01b,
            Recv = 10b,
        ],
    ],
}

// @todo replace with Event
register_bitfields! {
    u256,
    Notification [
        BoundTCB OFFSET(16) NUMBITS(48) [],
        MsgId OFFSET(64) NUMBITS(64) [],

        QueueHead OFFSET(144) NUMBITS(48) [],
        QueueTail OFFSET(192) NUMBITS(48) [],
        State OFFSET(254) NUMBITS(2) [
            Idle = 00b,
            Waiting = 01b,
            Active = 10b,
        ],
    ]
}

// TCB (Thread)
// +--VirtSpace
// +--CapSpace

enum MemoryKind {
    General,
    Device,
}

// The source of all available memory, device or general.
// Boot code reserves kernel memory and initial mapping allocations (4 pages probably - on rpi3? should be platform-dependent).
// The rest is converted to untypeds with appropriate kind and given away to start thread.

// Untyped.retype() derives cap to a typed cap (derivation tree must be maintained)

trait Untyped {
    // Uses T::SIZE_BITS to properly size the resulting object
    // in some cases size_bits must be passed as argument though...
    fn retype<T: NucleusObject>(target_cap: CapNodeRootedPath, target_cap_offset: usize, num_objects: usize) -> Result<CapSlice>; // @todo return an array of caps?
}

// with GATs
// trait Retyped { type Result = CapTable::<T> .. }

// MMU

// ActivePageTable (--> impl VirtSpace for ActivePageTable etc...)
// * translate(VirtAddr)->PhysAddr
// * translate_page(Page)->PhysAddr
// * map_to(Page, PhysFrame, Flags, FrameAllocator)->()
// * map(Page, Flags, FrameAllocator)->()
// * identity_map(PhysFrame, Flags, FrameAllocator)->()
// * unmap(Page, FrameAllocator)->()

trait VirtSpace {
    fn map(virt_space: VirtSpace/*Cap*/, vaddr: VirtAddr, rights: CapRights, attr: VMAttributes) -> Result<()>; /// ??
    fn unmap() -> Result<()>; /// ??
    fn remap(virt_space: VirtSpace/*Cap*/, rights: CapRights, attr: VMAttributes) -> Result<()>; /// ??
    fn get_address() -> Result<PhysAddr>;///??
}

// ARM AArch64 processors have a four-level page-table structure, where the
// VirtSpace is realised as a PageGlobalDirectory. All paging structures are
// indexed by 9 bits of the virtual address.

// AArch64 page hierarchy:
//
// PageGlobalDirectory (L0)  -- aka VirtSpace
// +--PageUpperDirectory (L1)
//    +--Page<Size1GiB> -- aka HugePage
//    |  or
//    +--PageDirectory (L2)
//       +--Page<Size2MiB> -- aka LargePage
//       |  or
//       +--PageTable (L3)
//          +--Page<Size4KiB> -- aka Page


/// Cache data management.
trait PageCacheManagement {
    /// Cleans the data cache out to RAM.
    /// The start and end are relative to the page being serviced.
    fn clean_data(start_offset: usize, end_offset: usize) -> Result<()>;
    /// Clean and invalidates the cache range within the given page.
    /// The range will be flushed out to RAM. The start and end are relative
    /// to the page being serviced.
    fn clean_invalidate_data(start_offset: usize, end_offset: usize) -> Result<()>;
    /// Invalidates the cache range within the given page.
    /// The start and end are relative to the page being serviced and should
    /// be aligned to a cache line boundary where possible. An additional
    /// clean is performed on the outer cache lines if the start and end are
    /// not aligned, to clean out the bytes between the requested and
    /// the cache line boundary.
    fn invalidate_data(start_offset: usize, end_offset: usize) -> Result<()>;
    /// Cleans data lines to point of unification, invalidates
    /// corresponding instruction lines to point of unification, then
    /// invalidates branch predictors.
    /// The start and end are relative to the page being serviced.
    fn unify_instruction_cache(start_offset: usize, end_offset: usize) -> Result<()>;
}

// ARM
// mod aarch64 {

struct Page {}

impl Page {
    // VirtSpace-like interface.
    /// Get the physical address of the underlying frame.
    fn get_address() -> Result<PhysAddr> { todo!() }
    fn map(virt_space: VirtSpace/*Cap*/, vaddr: VirtAddr, rights: CapRights, attr: VMAttributes) -> Result<()> { todo!() }
    /// Changes the permissions of an existing mapping.
    fn remap(virt_space: VirtSpace/*Cap*/, rights: CapRights, attr: VMAttributes) -> Result<()> { todo!() }
    fn unmap() -> Result<()> { todo!() }
    // MMIO space.
    fn map_io(iospace: IoSpace/*Cap*/, rights: CapRights, ioaddr: VirtAddr) -> Result<()> { todo!() }
}

impl PageCacheManagement for Page {
    fn clean_data(start_offset: usize, end_offset: usize) -> _ {
        todo!()
    }

    fn clean_invalidate_data(start_offset: usize, end_offset: usize) -> _ {
        todo!()
    }

    fn invalidate_data(start_offset: usize, end_offset: usize) -> _ {
        todo!()
    }

    fn unify_instruction_cache(start_offset: usize, end_offset: usize) -> _ {
        todo!()
    }
}

// ARM
// L3 tables
struct PageTable {}

impl PageTable {
    fn map(virt_space: VirtSpace/*Cap*/, vaddr: VirtAddr, attr: VMAttributes) -> Result<()> { todo!() }
    fn unmap() -> Result<()> { todo!() }
}

// AArch64 - probably just impl some Mapping trait for these "structs"?
// L2 table
struct PageDirectory {}

impl PageDirectory {
    fn map(pud: PageUpperDirectory/*Cap*/, vaddr: VirtAddr, attr: VMAttributes) -> Result<()> { todo!() }
    fn unmap() -> Result<()> { todo!() }
}

// L1 table
struct PageUpperDirectory {}

impl PageUpperDirectory {
    fn map(pgd: PageGlobalDirectory/*Cap*/, vaddr: VirtAddr, attr: VMAttributes) -> Result<()> { todo!() }
    fn unmap() -> Result<()> { todo!() }
}

// L0 table
struct PageGlobalDirectory {
    // @todo should also impl VirtSpace to be able to map shit?
    // or the Page's impl will do this?
}

impl PageCacheManagement for PageGlobalDirectory {
    fn clean_data(start_offset: usize, end_offset: usize) -> ! {
        todo!()
    }

    fn clean_invalidate_data(start_offset: usize, end_offset: usize) -> ! {
        todo!()
    }

    fn invalidate_data(start_offset: usize, end_offset: usize) -> ! { todo!() }

    fn unify_instruction_cache(start_offset: usize, end_offset: usize) -> ! {
        todo!()
    }
}

// implemented for x86 and arm
trait ASIDPool {
    fn assign(virt_space: VirtSpace/*Cap*/) -> Result<()>;
}

// implemented for x86 and arm
trait ASIDControl {
    fn make_pool(untyped: Untyped, target_cap_space_cap: CapNodeRootedPath) -> Result<()>;
}

// Allocation details

// 1. should be possible to map non-SAS style
// 2. should be easy to map SAS style
// 3. should not allocate any memory dynamically
//    ^ problem with the above API is FrameAllocator
//    ^ clients should supply their own memory for frames... from FrameCaps


// https://github.com/seL4/seL4_libs/tree/master/libsel4allocman

// Allocation overview

// Allocation is complex due to the circular dependencies that exist on allocating resources. These dependencies are loosely described as

//     Capability slots: Allocated from untypeds, book kept in memory.
//     Untypeds / other objects (including frame objects): Allocated from other untypeds, into capability slots, book kept in memory.
//     memory: Requires frame object.

// Other seL4-like kernel objects and their interfaces:

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

trait Notification {
    fn signal(dest: Cap);
    fn wait(src: Cap) -> Result<Option<&Badge>>;
    fn poll(cap: Cap) -> Result<(MessageInfo, Option<&Badge>)>;
}

trait IRQHandler {
    fn set_notification(notification: CapNode) -> Result<()>;
    fn ack() -> Result<()>;
    fn clear() -> Result<()>;
}

trait IRQControl {
    fn get(irq: u32, dest: CapNodeRootedPath) -> Result<()>;
    // ARM?
    fn get_trigger();
    fn get_trigger_core();
}

// Syscalls (kernel API)
trait API {
    fn send(cap: Cap, msg_info: MessageInfo);
    // Wait for message, when it is received,
    // return object Badge and block caller on `reply`.
    fn recv(src: Cap, reply: Cap) -> Result<(MessageInfo, Option<&Badge>)>;
    fn call(cap: Cap, msg_info: MessageInfo) -> Result<(MessageInfo, Option<&Badge>)>;
    fn reply(msg_info: MessageInfo);
    fn nb_send(dest: Cap, msg_info: MessageInfo);
    // As Recv but invoke `reply` first.
    fn reply_recv(src: Cap, reply: Cap, msg_info: MessageInfo) -> Result<(MessageInfo, Option<&Badge>)>;
    // As ReplyRecv but invoke `dest` not `reply`.
    fn nb_send_recv(dest: Cap, msg_info: MessageInfo, src: Cap, reply: Cap) -> Result<(MessageInfo, Options<&Badge>)>;
    fn nb_recv(src: Cap) -> Result<(MessageInfo, Option<&Badge>)>;
    // As NBSendRecv, with no reply. Donation is not possible.
    fn nb_send_wait(cap: Cap, msg_info: MessageInfo, src: Cap) -> Result<(MessageInfo, Option<&Badge>)>;
    // As per Recv, but donation not possible.
    fn wait(src: Cap) -> Result<(MessageInfo, Option<&Badge>)>;
    fn r#yield();
    // Plus some debugging calls...
}

struct Nucleus {}

impl API for Nucleus {
    fn send(cap: _, msg_info: _) {
        unimplemented!()
    }

    fn recv(src: _, reply: _) -> _ {
        unimplemented!()
    }

    fn call(cap: _, msg_info: _) -> _ {
        unimplemented!()
    }

    fn reply(msg_info: _) {
        unimplemented!()
    }

    fn nb_send(dest: _, msg_info: _) {
        unimplemented!()
    }

    fn reply_recv(src: _, reply: _, msg_info: _) -> _ {
        unimplemented!()
    }

    fn nb_send_recv(dest: _, msg_info: _, src: _, reply: _) -> _ {
        unimplemented!()
    }

    fn nb_recv(src: _) -> _ {
        unimplemented!()
    }

    fn nb_send_wait(cap: _, msg_info: _, src: _) -> _ {
        unimplemented!()
    }

    fn wait(src: _) -> _ {
        unimplemented!()
    }

    fn r#yield() {
        unimplemented!()
    }
}

trait DomainSet {
    // ??
    fn set(domain: Dom, thread: TCB);
}

// Virtualisation
// ARM
trait VCPU {
    fn inject_irq(virq: u16, priority: u8, group: u8, index: u8) -> Result<()>;
    fn read_registers();
    fn write_registers();
    fn set_tcb();
}
