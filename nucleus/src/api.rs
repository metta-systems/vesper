/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Syscall API for calling kernel functions.
//!
//! Arch-specific kernel ABI decodes syscall invocations and calls API functions to perform actual
//! operations.

use vesper_user::SysCall as SysCall;

// Syscalls (kernel API)
trait API {
    // Three below (send, nb_send, call) are "invocation" syscalls.
    fn send(cap: Cap, msg_info: MessageInfo);
    fn nb_send(dest: Cap, msg_info: MessageInfo);
    fn call(cap: Cap, msg_info: MessageInfo) -> Result<(MessageInfo, Option<&Badge>)>;
    // Wait for message, when it is received,
    // return object Badge and block caller on `reply`.
    fn recv(src: Cap, reply: Cap) -> Result<(MessageInfo, Option<&Badge>)>;
    fn reply(msg_info: MessageInfo);
    // As Recv but invoke `reply` first.
    fn reply_recv(
        src: Cap,
        reply: Cap,
        msg_info: MessageInfo,
    ) -> Result<(MessageInfo, Option<&Badge>)>;
    fn nb_recv(src: Cap) -> Result<(MessageInfo, Option<&Badge>)>;
    fn r#yield();
    // -- end of default seL4 syscall list --
    // As ReplyRecv but invoke `dest` not `reply`.
    fn nb_send_recv(
        dest: Cap,
        msg_info: MessageInfo,
        src: Cap,
        reply: Cap,
    ) -> Result<(MessageInfo, Options<&Badge>)>;
    // As NBSendRecv, with no reply. Donation is not possible.
    fn nb_send_wait(
        cap: Cap,
        msg_info: MessageInfo,
        src: Cap,
    ) -> Result<(MessageInfo, Option<&Badge>)>;
    // As per Recv, but donation not possible.
    fn wait(src: Cap) -> Result<(MessageInfo, Option<&Badge>)>;
    // Plus some debugging calls...
}

fn handle_syscall(syscall: SysCall) -> Result<()> {
    match syscall {
        SysCall::Send => {
            let result = handle_invocation(false, true);
            if result.is_err() {
                let irq = get_active_irq();
                if irq.is_ok() {
                    handle_interrupt(irq.unwrap());
                }
            }
        }
        SysCall::NBSend => {
            let result = handle_invocation(false, false);
            if result.is_err() {
                let irq = get_active_irq();
                if irq.is_ok() {
                    handle_interrupt(irq.unwrap());
                }
            }
        }
        SysCall::Call => {
            let result = handle_invocation(true, true);
            if result.is_err() {
                let irq = get_active_irq();
                if irq.is_ok() {
                    handle_interrupt(irq.unwrap());
                }
            }
        }
        SysCall::Recv => handle_receive(true),
        SysCall::Reply => handle_reply(),
        SysCall::ReplyRecv => {
            handle_reply();
            handle_receive(true)
        }
        SysCall::NBRecv => handle_receive(false),
        SysCall::Yield => handle_yield(),
    }

    schedule();
    activateThread();

    Ok(())
}

fn handle_invocation(is_call: bool, is_blocking: bool) -> Result<()> {
    let thread: &TCB = KernelCurrentThread;

    let infoRegister = thread.get_register(msgInfoRegister);
    let info: MessageInfo = messageInfoFromWord(infoRegister);
    let cap_ptr: CapPath = thread.get_register(capRegister);

    result = thread.lookup_cap_and_slot(cap_ptr);

    if result.is_err() {
        println!(
            "<<vesper[T{} \"{}\" @{}]: Invocation of invalid cap {}.>>",
            thread,
            thread.name,
            thread.get_restart_pc(),
            cap_ptr,
        );

        if is_blocking {
            handle_fault(thread);
        }

        return Ok(());
    }

    let buffer = thread.lookup_ipc_buffer(false);

    let status = thread.lookup_extra_caps(buffer, info);

    if status.is_err() {
        println!(
            "<<vesper[T{} \"{}\" @{}]: Lookup of extra caps failed.>>",
            thread,
            thread.name,
            thread.get_restart_pc(),
        );

        if is_blocking {
            handle_fault(thread);
        }

        return Ok(());
    }

    let mut length = info.length();
    if length > n_MsgRegisters && !buffer {
        length = n_MsgRegisters;
    }

    let status = decode_invocation(
        info.label(),
        length,
        cap_ptr,
        result.slot,
        result.cap,
        current_extra_caps,
        is_blocking,
        is_call,
        buffer,
    );

    if status.is_err() {
        return match status {
            Err(Preempted) => status,
            Err(SysCallError) => {
                if is_call {
                    thread.replyFromKernel_error();
                }
                Ok(())
            }
        };
    }

    if thread.get_state() == ThreadState::Restart {
        if is_call {
            thread.replyFromKernel_success_empty();
        }
        thread.set_state(ThreadState::Running);
    }

    Ok(())
}

fn handle_receive(is_blocking: bool) {
    let endpoint_cap_ptr = KernelCurrentThread.get_register(capRegister);

    let result = KernelCurrentThread.lookup_cap(endpoint_cap_ptr);

    if result.is_err() {
        KernelCurrentFault = Fault_CapFault::new(endpoint_cap_ptr, true);
        handle_fault(KernelCurrentThread);
        return Ok(());
    }

    match result.cap.get_type() {
        endpoint => ,
        notification => ,
        _ => fault,
    }
}

fn handle_reply() {
    let caller_slot = KernelCurrentThread.get_caller_slot();
    let caller_cap = caller_slot.capability;
    match caller_cap.get_type() {
        ReplyCap::Type.value => {
            // if (cap_reply_cap_get_capReplyMaster(callerCap)) {
            //     break;
            // }
            // caller = ((tcb_t *)(cap_reply_cap_get_capTCBPtr(callerCap)));
            // if(!(caller != ksCurThread)) _assert_fail("caller must not be the current thread", "src/api/syscall.c", 313, __FUNCTION__);
            // do_reply_transfer(ksCurThread, caller, callerSlot);
        },
        NullCap::Type.value => {
            println!("<<vesper[T{} \"{}\" @{}]: Attempted reply operation when no reply capability present.>>", KernelCurrentThread, KernelCurrentThread.name, KernelCurrentThread.get_restart_pc());
        },
        _ => {
            panic!("<<vesper[T{} \"{}\" @{}]: Invalid caller capability.>>", KernelCurrentThread, KernelCurrentThread.name, KernelCurrentThread.get_restart_pc());
        }
    }
}

fn do_reply_transfer() {}

fn handle_yield() {
    Scheduler::dequeue(KernelCurrentThread);
    Scheduler::append(KernelCurrentThread);
    Scheduler::reschedule_required();
}

#[derive(Debug, Snafu)]
enum Fault {
    #[snafu(display("null fault"))]
    Null,
    #[snafu(display("capability fault in {} phase at address {:x}", if in_receive_phase { "receive" } else { "send" }, address))]
    Capability {
        in_receive_phase: bool,
        address: PhysAddr,
    },
    #[snafu(display("vm fault on {} at address {:x} with status {:x}", if is_instruction_fault { "code" } else { "data" }, address, fsr))]
    VM {
        is_instruction_fault: bool,
        address: PhysAddr,
        fsr: u64, // status
    },
    #[snafu(display("unknown syscall {:x}", syscall_number))]
    UnknownSyscall {
        syscall_number: u64,
    },
    #[snafu(display("user exception {:x} code {:x}", number, code))]
    UserException {
        number: u64,
        code: u64,
    },
}

fn handle_fault(thread: &TCB) {
    let fault = KernelCurrentFault;

    let result = thread.send_fault_ipc();
    if result.is_err() {
        handle_double_fault(thread, fault);
    }
}

fn handle_double_fault(thread: &TCB, fault1: Fault) {
    let fault2 = KernelCurrentFault;

    println!("Caught {} while trying to handle {}", fault2, fault1);
    println!("in thread T{} \"{}\"", thread, thread.name);
    println!("at address {}", thread.get_restart_pc());
    println!("with stack trace:");
    arch::user_stack_trace(thread);

    thread.set_state(ThreadState::Inactive);
}

fn handle_unknown_syscall() {
    // handles
    // - SysDebugPutChar
    // - SysDebugHalt
    // - SysDebugSnapshot
    // - SysDebugCapIdentify
    // - SysDebugNameThread
    // - Fault_UnknownSyscall
}

fn handle_interrupt_entry() -> Result<()> {
    let irq = get_active_irq();
    if irq.is_ok() {
        handle_interrupt(irq.unwrap());
    } else {
        handle_spurious_irq();
    }

    schedule();
    activate_thread();

    Ok(())
}

/* TCB: size 64 bytes + sizeof(arch_tcb_t) (aligned to nearest power of 2) */
struct TCB {
    arch_tcb: arch::TCB,
    state: ThreadState, // 12 bytes?
    /* Notification that this TCB is bound to. If this is set, when this TCB waits on
     * any sync endpoint, it may receive a signal from a Notification object.
     * 4 bytes*/
    // notification_t *tcbBoundNotification;
    fault: Fault, // 8 bytes?
    lookup_failure: LookupFault, // 8 bytes
    /* Domain, 1 byte (packed to 4) */
    domain: Domain,
    /*  maximum controlled priorioty, 1 byte (packed to 4) */
    mcp: Priority,
    /* Priority, 1 byte (packed to 4) */
    priority: Priority,
    /* Timeslice remaining, 4 bytes */
    time_slice: u32,
    /* Capability pointer to thread fault handler, 4 bytes */
    fault_handler: CapPath,
    /* userland virtual address of thread IPC buffer, 4 bytes */
    ipc_buffer: VirtAddr,
    /* Previous and next pointers for scheduler queues , 8 bytes */
    sched_next: *mut TCB,
    sched_prev: *mut TCB,
    /* Previous and next pointers for endpoint and notification queues, 8 bytes */
    ep_next: *mut TCB,
    ep_prev: *mut TCB,
    /* Use any remaining space for a thread name */
    name: &str,
}

impl TCB {
    fn get_register(...) {}
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
}
//handleSyscall(syscall) in the slowpath()
// these are expressed in terms of
// handleInvocation(bool isCall, bool isBlocking)
// handleRecv(block)
// handleReply()
// replyRecv: -- handleReply+handleRecv
// handleYield()

// slowpath() called in c_handle_syscall() in abi
// Call and ReplyRecv have fastpath handlers
// the rest goes through slowpath

// c_handle_syscall called directly from SWI vector entry

struct Nucleus {}

impl API for Nucleus {
    //...
}
