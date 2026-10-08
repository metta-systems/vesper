/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Vesper single-address-space nanokernel.
//!
//! This crate implements the kernel binary proper.

#![no_std]
#![no_main]
#![feature(decl_macro)]
#![feature(allocator_api)]
#![feature(format_args_nl)]
#![feature(likely_unlikely)]
#![feature(stmt_expr_attributes)]
#![feature(slice_ptr_get)]
#![deny(missing_docs)]
#![deny(warnings)]
#![allow(unused)]
#![allow(internal_features)]
#![allow(linker_messages)]
#![feature(ptr_internals)]
#![feature(core_intrinsics)]

use {
    cfg_if::cfg_if,
    core::{
        arch::asm,
        panic::PanicInfo,
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    },
    libcpu::endless_sleep,
    libexception::arch::aarch64::{ExceptionContext, ExceptionOrigin, SavedContext},
    liblocking::{IRQSafeNullLock, interface::Mutex},
    liblog::{info, println, warn},
    libmapping::AccessPermissions,
    libobject::{
        ArchType, CapError, KeySlot, RawKey,
        fault::{FaultInfo, FaultKind},
        syscall_status,
    },
    libqemu::semihosting as semi,
    nucleus::objects::{
        ArchObjects, ArchObjectsImpl, Nucleus,
        access::{Access, ObjectId},
        completion::PendingKind,
        fault::FaultDelivery,
    },
};

// TODO: Split this into read-only part, that does not need locks, per-cpu mutable part that does not need locks,
// TODO: Shared atomic counters that do not need locks and shared mutable collections that DO need locks (but should be minority)

/// The Great Kernel Lock. The boot-carved [`Nucleus`] is accessed under it.
static KERNEL_LOCK: IRQSafeNullLock<()> = IRQSafeNullLock::new(());

/// Anchor to the boot-carved [`Nucleus`].
///
/// Written once by Kickstart (the one-time boot code) before the nucleus runs;
/// the inert nucleus only reads it. Kept as an atomic pointer so boot code can
/// record it via [`nucleus_set_anchor`] without the compiler constant-folding
/// the read (the static is only ever written from boot code).
static NUCLEUS: AtomicUsize = AtomicUsize::new(0);

/// Record the boot-carved [`Nucleus`] address.
///
/// Called once by Kickstart before the nucleus runs; the inert nucleus performs
/// no other initialization. Exported so the compiler cannot constant-fold the
/// anchor read (the static is only ever written from boot code).
///
/// # Safety
/// Must be called exactly once, before any syscall, from the single boot core
/// with interrupts masked. `ptr` must point at a live, exclusively-owned
/// boot-carved [`Nucleus`].
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.bootstrap")]
pub unsafe extern "C" fn nucleus_set_anchor(ptr: *mut Nucleus<nucleus::objects::ArchObjectsImpl>) {
    // SAFETY: one-shot boot write, single-core, before any syscall.
    unsafe {
        NUCLEUS.store(ptr as usize, Ordering::Relaxed);
    }
}

/// The boot-carved [`Nucleus`], or `None` before Kickstart records it.
pub fn nucleus_anchor() -> Option<*mut Nucleus<nucleus::objects::ArchObjectsImpl>> {
    let addr = NUCLEUS.load(Ordering::Relaxed);
    if addr == 0 {
        return None;
    }
    // SAFETY: Kickstart wrote `addr` as the address of the boot-carved Nucleus.
    Some(unsafe { addr as *mut Nucleus<nucleus::objects::ArchObjectsImpl> })
}

#[panic_handler]
fn panicked(info: &PanicInfo) -> ! {
    // Route panic output through the console logger: without an installed
    // logger, liblog drops every message on the default NopLogger, so a
    // nucleus panic would silently degrade into a bare hang. In QEMU builds
    // the logger mirrors to the semihosting console; without a registered
    // UART console the console write itself is a null-sink no-op. The only
    // install error is "a logger is already installed", which is fine here.
    let _logger = libconsole::init_logger();
    libmachine::panic::handler(info)
}

//------------------------------------------------------------------------------
//------------------------------------------------------------------------------
// Exception handlers
//------------------------------------------------------------------------------
//------------------------------------------------------------------------------

/// The default exception handler, invoked for every exception type unless the handler
/// is overridden.
/// Prints verbose information about the exception and then panics.
///
/// Default pointer is configured in the linker script.
#[unsafe(no_mangle)]
extern "C" fn default_exception_handler(exc: &ExceptionContext) {
    panic!(
        "Unexpected CPU Exception!\n\n\
        {exc}"
    );
}

//------------------------------------------------------------------------------
// Current, EL0
//------------------------------------------------------------------------------

#[unsafe(no_mangle)]
extern "C" fn current_el0_synchronous(e: &mut ExceptionContext) {
    // This vector is current EL using SP_EL0 (trusted EL1t), not lower EL0.
    // Exception entry has selected the shared SP_EL1 trap stack.
    current_elx_synchronous(e);
}

#[unsafe(no_mangle)]
extern "C" fn current_el0_irq(e: &mut ExceptionContext) {
    current_elx_irq(e);
}

#[unsafe(no_mangle)]
extern "C" fn current_el0_serror(e: &mut ExceptionContext) {
    current_elx_serror(e);
}

//------------------------------------------------------------------------------
// Current, ELx
//------------------------------------------------------------------------------

#[cfg(not(any(test, feature = "test_build")))]
#[unsafe(no_mangle)]
extern "C" fn current_elx_synchronous(e: &mut ExceptionContext) {
    // Only an SVC in AArch64 state is a capability invocation. Any other
    // synchronous exception from the current EL (data/instruction abort,
    // alignment fault, undefined instruction) must not be decoded as a
    // syscall: its context would re-execute the faulting instruction with
    // the "syscall result" written into x0..x2, looping forever. Route it
    // to the default handler, which reports the exception and halts.
    #[cfg(feature = "fp_trap_test")]
    if absorb_fp_simd_trap(e) {
        return;
    }
    if !is_aarch64_svc() {
        default_exception_handler(e);
    }
    cap_invoke_handler(e);
}

#[cfg(any(test, feature = "test_build"))]
#[unsafe(no_mangle)]
extern "C" fn current_elx_synchronous(e: &mut ExceptionContext) {
    {
        use aarch64_cpu::registers::{ESR_EL1, Readable};

        const TEST_SVC_ID: u64 = 0x1337;

        let esr_el1 = libexception::arch::esr_el1::EsrEL1(LocalRegisterCopy::new(ESR_EL1.get()));

        if let Some(ESR_EL1::EC::Value::SVC64) = esr_el1.exception_class()
            && esr_el1.iss() == TEST_SVC_ID
        {
            liblog::println!("Serving syscall {TEST_SVC_ID}");
            return;
        }
    }

    if libdebug::exception_dump(e) {
        return;
    }

    default_exception_handler(e);
}

#[unsafe(no_mangle)]
extern "C" fn current_elx_irq(e: &mut ExceptionContext) {
    // -- @todo
    // let token = unsafe { &exception::asynchronous::IRQContext::new() };
    // exception::asynchronous::irq_manager().handle_pending_irqs(token);
    default_exception_handler(e);
}

#[unsafe(no_mangle)]
extern "C" fn current_elx_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

//------------------------------------------------------------------------------
// Lower, AArch64
//------------------------------------------------------------------------------

#[unsafe(no_mangle)]
extern "C" fn lower_aarch64_synchronous(e: &mut ExceptionContext) {
    // Only an SVC is a capability invocation; every other synchronous
    // exception from EL0 is a fault, delivered to the fault handler.
    if !is_aarch64_svc() {
        use aarch64_cpu::registers::{ESR_EL1, FAR_EL1, Readable};

        let faulting = e.save();
        let info = FaultInfo {
            kind: FaultKind::CpuException,
            esr: ESR_EL1.get(),
            far: FAR_EL1.get(),
            pc: faulting.elr_el1,
            sp: faulting.sp,
            depth: current_invocation_depth(),
        };
        deliver_fault(e, &faulting, info);
        return;
    }
    cap_invoke_handler(e);
}

/// The current Thread's invocation depth, for fault reports (0 if unknown).
fn current_invocation_depth() -> u64 {
    KERNEL_LOCK
        .lock(|()| {
            let nucleus = nucleus_anchor()?;
            // SAFETY: the anchor names the live Nucleus; the lock serializes
            // this read.
            let nucleus = unsafe { &*nucleus };
            let index = usize::try_from(nucleus.current_thread?).ok()?;
            let thread = nucleus.pools.threads.get_live(index)?;
            u64::try_from(thread.invocation_stack.len()).ok()
        })
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
extern "C" fn lower_aarch64_irq(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[unsafe(no_mangle)]
extern "C" fn lower_aarch64_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

//------------------------------------------------------------------------------
// Lower, AArch32
//------------------------------------------------------------------------------

#[unsafe(no_mangle)]
extern "C" fn lower_aarch32_synchronous(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[unsafe(no_mangle)]
extern "C" fn lower_aarch32_irq(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[unsafe(no_mangle)]
extern "C" fn lower_aarch32_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

/// Test-only hook for trusted `EL1t` code (`fp-trap-test`): faults in `EL1t` code
/// halt the kernel (fault delivery covers EL0 only), so to check that FP/SIMD
/// traps at `EL1t` this absorbs an FP/SIMD access trap (`ESR_EL1.EC` 0x07) by
/// handing its syndrome back to the faulting code in `x0` and resuming after
/// the trapping instruction. Production builds halt on it like any `EL1t` fault.
#[cfg(feature = "fp_trap_test")]
fn absorb_fp_simd_trap(e: &mut ExceptionContext) -> bool {
    use aarch64_cpu::registers::{ESR_EL1, Readable};

    if !ESR_EL1.matches_all(ESR_EL1::EC::TrappedFP) {
        return false;
    }
    semi::println!(
        "🧪 FP/SIMD access trapped from {:?} at {:#x}, ESR {:#x}",
        e.origin,
        e.elr_el1,
        ESR_EL1.get()
    );
    e.gpr[0] = ESR_EL1.get();
    // Every A64 instruction is four bytes; the trap is taken before it executes.
    e.elr_el1 += 4;
    true
}

/// Whether the synchronous exception being handled is an SVC instruction
/// executed in `AArch64` state — the only exception class that carries a
/// capability invocation. The SVC immediate is deliberately ignored: there is
/// one syscall, so every `svc #n` is the same invocation (see the contract's
/// ordinary control invocation baseline).
fn is_aarch64_svc() -> bool {
    use aarch64_cpu::registers::{ESR_EL1, Readable};

    // ESR_EL1 bits 31:26 hold the exception class; 0x15 is an SVC
    // instruction executed in AArch64 state (see
    // `libexception::arch::esr_el1::ESR_EL1::EC::Value::SVC64`).
    const EC_SVC64: u64 = 0x15;
    (ESR_EL1.get() >> 26) & 0x3F == EC_SVC64
}

//------------------------------------------------------------------------------
// Kernel entry point
//------------------------------------------------------------------------------

use owo_colors::OwoColorize;

#[unsafe(no_mangle)]
extern "C" fn cap_invoke_handler(frame: &mut ExceptionContext) {
    let key = RawKey::from_wire(frame.gpr[0]);
    let op = frame.gpr[1];
    semi::println!(
        "➡️  {}(key: {key:?}, op: {op}) @ PC {:#016X}, SP {:#016X}, exception frame @ {:#016X}",
        "CapInvoke SYSCALL".dimmed(),
        get_pc(),
        get_sp(),
        core::ptr::from_mut(frame) as u64,
    );

    // SP_EL1 belongs to the active kernel, not a schedulable caller. Gate
    // provenance before dispatch can reserve a pending record or mutate queues.
    if !matches!(
        frame.origin,
        ExceptionOrigin::CurrentSp0 | ExceptionOrigin::LowerAarch64
    ) {
        let (status, detail0, detail1) = CapError::InvalidDomain.code();
        frame.gpr[0] = status;
        frame.gpr[1] = detail0;
        frame.gpr[2] = detail1;
        return;
    }

    // Dispatch reads every input from this copy of the saved frame,
    // including PPC Call's x9 target SP; nothing after Rust entry uses live
    // argument registers.
    let saved = frame.save();

    // SAFETY: Unsafe.
    let outcome = unsafe {
        #[allow(static_mut_refs)]
        KERNEL_LOCK.lock(|()| {
            let Some(ptr) = nucleus_anchor() else {
                panic!("nucleus not booted by Kickstart")
            };
            // SAFETY: the anchor points at the live boot-carved Nucleus.
            nucleus::api::handle_cap_invoke(unsafe { &mut *ptr }, &saved)
        })
    };

    // A blocked invocation cannot return to its caller yet. Copy its state
    // into the Thread, then rewrite this transient frame for the selected
    // Thread. The handler unwinds normally; vectors restore it with ERET.
    // Dispatch guards have ended before the scheduling transaction begins.
    let (x0, x1, x2) = match outcome {
        Ok(nucleus::api::InvokeOutcome::Complete((v0, v1))) => (syscall_status::SUCCESS, v0, v1),
        Ok(nucleus::api::InvokeOutcome::Activate(prepared)) => {
            // API/Access guards and KERNEL_LOCK have ended. Execution and
            // SP_EL1 use the invariant high TTBR1 map, not the replaced root.
            // Trap entry masks interrupts; no scheduling/reentry occurs before
            // this immediate installation on the enforced single boot core.
            ArchObjectsImpl::install_translation_context(prepared.root(), prepared.asid());
            semi::println!("{}", "✅ AddressSpace::Activate()".on_cyan());
            (syscall_status::SUCCESS, 0, 0)
        }
        Ok(nucleus::api::InvokeOutcome::Blocked(record)) => {
            park_and_resume(frame, record);
            return;
        }
        Ok(nucleus::api::InvokeOutcome::Call(committed)) => {
            // Committed under the lock: continuation pushed, Thread migrated.
            // Guards and KERNEL_LOCK have ended; install the target context,
            // then rewrite this transient frame with the scrubbed target
            // entry. Vectors ERET into the target on the same Thread.
            let translation = committed.translation;
            ArchObjectsImpl::install_translation_context(translation.root(), translation.asid());
            frame.restore(committed.target);
            semi::println!("{}", "✅ Invocation::Call()".on_cyan());
            return;
        }
        Ok(nucleus::api::InvokeOutcome::Return(committed)) if committed.terminate => {
            // A fault handler chose to terminate its faulting Thread.
            semi::println!("{}", "✅ Thread::Return()".on_cyan());
            terminate_faulted(frame, &committed.resumed);
            return;
        }
        Ok(nucleus::api::InvokeOutcome::Fault(kind)) => {
            // A Return protocol fault, delivered like any EL0 fault. Faults in
            // trusted EL1t code halt the kernel.
            assert!(
                frame.origin == ExceptionOrigin::LowerAarch64,
                "Thread.Return fault in trusted EL1t code: {kind:?}"
            );
            // The fault is at the Return's `svc`: ELR already points past it.
            let mut faulting = saved;
            faulting.elr_el1 = faulting.elr_el1.wrapping_sub(4);
            let info = FaultInfo {
                kind,
                esr: 0,
                far: 0,
                pc: faulting.elr_el1,
                sp: faulting.sp,
                depth: current_invocation_depth(),
            };
            deliver_fault(frame, &faulting, info);
            return;
        }
        Ok(nucleus::api::InvokeOutcome::Return(committed)) => {
            // Committed under the lock: top continuation popped, Thread back
            // in its source AddressSpace. Install, then resume the source with
            // SUCCESS/r0/r1 and its exact saved callee-saved state (or, ending
            // a fault handler, the faulting state as its action selects).
            let translation = committed.translation;
            ArchObjectsImpl::install_translation_context(translation.root(), translation.asid());
            frame.restore(committed.resumed);
            semi::println!("{}", "✅ Thread::Return()".on_cyan());
            return;
        }
        Err(e) => e.code(),
    };
    // Return values
    semi::println!(
        "⬅️  {}(Return {x0:#x}, {x1:#x}, {x2:#x})",
        "CapInvoke SYSCALL".dimmed()
    );
    frame.gpr[0] = x0;
    frame.gpr[1] = x1;
    frame.gpr[2] = x2;
}

// ═══════════════════════════════════════════════════════════════════
// CONTEXT SWITCHING (completion foundation, 2026-09-16)
// ═══════════════════════════════════════════════════════════════════

/// Park the current Thread in private storage and select a runnable context.
///
/// Dispatch has released its guards before this transaction begins. No frame
/// address survives it: the current per-core frame is rewritten after the
/// lock ends, and the Rust handler returns normally to the vector epilogue.
/// Historical prerequisite status, superseded by the implementation below:
/// hardware `AddressSpace` installation was pending, with the trusted Bounce
/// fixture running under the bootstrap translation root.
/// Implementation status: checked preparation/commit now lives in
/// `Nucleus::park_and_select`; installation follows the lock below. Bootstrap
/// must provision complete source/target roots before admitting the first wait.
fn park_and_resume(frame: &mut ExceptionContext, record: ObjectId) {
    assert!(
        frame.origin != ExceptionOrigin::CurrentSpx,
        "kernel-mode execution cannot retain a continuation on SP_EL1"
    );
    let saved = frame.save();
    let resumed = KERNEL_LOCK.lock(|()| {
        let Some(nucleus_ptr) = nucleus_anchor() else {
            panic!("nucleus not booted by Kickstart")
        };
        // SAFETY: the anchor names the retained boot-carved Nucleus, and the
        // kernel lock gives exclusive access for this scheduling transaction.
        let nucleus = unsafe { &mut *nucleus_ptr };
        // SAFETY: exclusive access is serialized by KERNEL_LOCK, dispatch's
        // Access has ended, and no second context overlaps this transaction.
        let access = unsafe { Access::new() };
        nucleus.park_and_select(&access, saved, record)
    });

    // No timer exists to wake a wholly blocked fixture: halt honestly,
    // rather than returning fake success to a blocked caller. Scheduler-error
    // recovery/fault delivery has no ABI yet; trusted fixture corruption is an
    // invariant failure, after a failure-atomic preparation (not a lost wait).
    let resumed = resumed.unwrap_or_else(|error| {
        panic!(
            "wait/resume scheduling invariant failed before commit: {:?}",
            error.code()
        )
    });
    resume_selected(frame, &resumed, "parked");
}

/// Install and enter the Thread a scheduling transaction selected: rewrite
/// the transient frame with its context and trace the switch. `why` names
/// what happened to the Thread that stopped.
fn resume_selected(
    frame: &mut ExceptionContext,
    resumed: &nucleus::objects::resume::PreparedResume,
    why: &str,
) {
    let current = resumed.current;
    let next = resumed.next;
    let restored = resumed.saved;
    // Only copied prepared metadata and the transient frame are live here;
    // no Thread or object guard crosses hardware installation or ERET.
    // High kernel code and shared high SP_EL1 remain mapped by TTBR1. The
    // single-core, masked trap path cannot retire/rebind between prepare and
    // install; this is not a reusable lifetime pin for asynchronous work.
    ArchObjectsImpl::install_translation_context(
        resumed.translation.root(),
        resumed.translation.asid(),
    );
    // SP_EL1 is reclaimed by normal unwinding, with no kernel stack switch.
    frame.restore(restored);
    if let Some(completion) = resumed.completion {
        let kind = match completion.kind {
            PendingKind::NotificationWait => "Notification::Wait",
            PendingKind::EventCountAwait => "EventCount::Await",
        };
        let status = completion.status;
        let result0 = completion.result0;
        if status == syscall_status::SUCCESS {
            semi::println!("✅ {kind}(0x{result0:x}) resumed");
        } else {
            semi::println!("⬅️ {kind} resumed with status {status:#x}");
        }
    }
    semi::println!(
        "🔄 context switch: thread {current} {why}, resuming thread {next} @ SP {:#x}, PC {:#x}",
        restored.sp,
        restored.elr_el1,
    );
}

// ═══════════════════════════════════════════════════════════════════
// FAULT DELIVERY
// ═══════════════════════════════════════════════════════════════════

/// Deliver a fault on the current Thread: a synchronous upcall into its
/// `AddressSpace`'s fault handler, or — if nobody takes it — park the Thread
/// as faulted and resume the next one. `faulting` is the complete faulting
/// state (for a Return fault, with PC at its `svc`).
fn deliver_fault(frame: &mut ExceptionContext, faulting: &SavedContext, info: FaultInfo) {
    semi::println!(
        "⚠️  fault {:?} at PC {:#x}, ESR {:#x}, FAR {:#x}, depth {}",
        info.kind,
        info.pc,
        info.esr,
        info.far,
        info.depth
    );
    let delivery = KERNEL_LOCK.lock(|()| {
        let Some(nucleus_ptr) = nucleus_anchor() else {
            panic!("nucleus not booted by Kickstart")
        };
        // SAFETY: the anchor names the retained boot-carved Nucleus; the
        // kernel lock gives exclusive access for this transaction.
        let nucleus = unsafe { &mut *nucleus_ptr };
        // SAFETY: exclusive access is serialized by KERNEL_LOCK and no other
        // Access or guard overlaps this transaction.
        let access = unsafe { Access::new() };
        nucleus.deliver_fault(&access, *faulting, info)
    });
    match delivery {
        Ok(FaultDelivery::Handler(committed)) => {
            let translation = committed.translation;
            ArchObjectsImpl::install_translation_context(translation.root(), translation.asid());
            frame.restore(committed.target);
            semi::println!("{}", "✅ fault delivered to its handler".on_cyan());
        }
        Ok(FaultDelivery::Unhandled(resumed)) => {
            semi::println!("⛔ unhandled fault: thread parked as faulted");
            resume_selected(frame, &resumed, "faulted");
        }
        // Nothing is left to run (or scheduling state is corrupt).
        Err(error) => panic!("fault left nothing to run: {:?}", error.code()),
    }
}

/// A fault handler returned with the terminate action: park the Thread as
/// faulted with its faulting state and resume the next one.
fn terminate_faulted(frame: &mut ExceptionContext, faulting: &SavedContext) {
    let resumed = KERNEL_LOCK.lock(|()| {
        let Some(nucleus_ptr) = nucleus_anchor() else {
            panic!("nucleus not booted by Kickstart")
        };
        // SAFETY: as in `deliver_fault`.
        let nucleus = unsafe { &mut *nucleus_ptr };
        // SAFETY: as in `deliver_fault`.
        let access = unsafe { Access::new() };
        let current = nucleus.current_thread.map(|index| index as usize);
        current
            .ok_or(CapError::InvalidDomain)
            .and_then(|current| nucleus.park_faulted(&access, current, *faulting))
    });
    match resumed {
        Ok(resumed) => {
            semi::println!("⛔ fault handler terminated the thread");
            resume_selected(frame, &resumed, "terminated");
        }
        Err(error) => panic!("fault left nothing to run: {:?}", error.code()),
    }
}

fn get_pc() -> u64 {
    let pc: u64;
    // SAFETY: Safe.
    unsafe {
        asm!(
            "adr {}, .",
            out(reg) pc,
        );
    }
    pc
}

fn get_sp() -> u64 {
    use aarch64_cpu::registers::Readable;
    aarch64_cpu::registers::SP.get()
}
