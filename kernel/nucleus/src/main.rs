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
    libexception::arch::aarch64::ExceptionContext,
    liblocking::{IRQSafeNullLock, interface::Mutex},
    liblog::{info, println, warn},
    libmapping::AccessPermissions,
    libobject::{ArchType, CapError, KeySlot, RawKey, syscall_status},
    libqemu::semihosting as semi,
    nucleus::objects::{
        ExecutionContext, Nucleus,
        access::ObjectId,
        completion::{PendingKind, PendingState},
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
extern "C" fn current_el0_synchronous(_e: &mut ExceptionContext) {
    panic!("Should not be here. Use of SP_EL0 in EL1 is not supported.")
}

#[unsafe(no_mangle)]
extern "C" fn current_el0_irq(_e: &mut ExceptionContext) {
    panic!("Should not be here. Use of SP_EL0 in EL1 is not supported.")
}

#[unsafe(no_mangle)]
extern "C" fn current_el0_serror(_e: &mut ExceptionContext) {
    panic!("Should not be here. Use of SP_EL0 in EL1 is not supported.")
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
    // See `current_elx_synchronous`: only an SVC is a capability invocation.
    if !is_aarch64_svc() {
        default_exception_handler(e);
    }
    cap_invoke_handler(e);
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

/// Whether the synchronous exception being handled is an SVC instruction
/// executed in `AArch64` state — the only exception class that carries a
/// capability invocation.
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

#[unsafe(no_mangle)]
extern "C" fn cap_invoke_handler(frame: &mut ExceptionContext) {
    let key = RawKey::from_wire(frame.gpr[0]);
    let op = frame.gpr[1];
    semi::println!(
        "➡️ CapInvoke SYSCALL(key: {key:?}, op: {op}) @ PC {:#016X}, SP {:#016X}, exception frame @ {:#016X}",
        get_pc(),
        get_sp(),
        core::ptr::from_mut(frame) as u64,
    );

    // semi::println!("{}", frame);

    let args = [
        frame.gpr[2],
        frame.gpr[3],
        frame.gpr[4],
        frame.gpr[5],
        frame.gpr[6],
        frame.gpr[7],
    ];

    // SAFETY: Unsafe.
    let outcome = unsafe {
        #[allow(static_mut_refs)]
        KERNEL_LOCK.lock(|()| {
            let Some(ptr) = nucleus_anchor() else {
                panic!("nucleus not booted by Kickstart")
            };
            // SAFETY: the anchor points at the live boot-carved Nucleus.
            nucleus::api::handle_cap_invoke(unsafe { &mut *ptr }, key, op, &args)
        })
    };

    // A blocked invocation does not return: park the caller and switch.
    // This happens after the kernel-lock closure has ended, satisfying the
    // contract's rule that scheduling occurs only after guards are released.
    let (x0, x1, x2) = match outcome {
        Ok(nucleus::api::InvokeOutcome::Complete((v0, v1))) => (syscall_status::SUCCESS, v0, v1),
        Ok(nucleus::api::InvokeOutcome::Blocked(record)) => {
            // SAFETY: the kernel lock is released, no access guards are held,
            // and the frame names the caller's saved exception context on the
            // current kernel stack.
            unsafe { park_and_switch(core::ptr::from_mut(frame) as u64, record) }
        }
        Err(e) => e.code(),
    };
    // Return values
    semi::println!("⬅️ CapInvoke SYSCALL(Return {x0:#x}, {x1:#x}, {x2:#x})");
    // SAFETY: Not safe.
    unsafe {
        frame.gpr[0] = x0;
        frame.gpr[1] = x1;
        frame.gpr[2] = x2;
    }
}

// ═══════════════════════════════════════════════════════════════════
// CONTEXT SWITCHING (completion foundation, 2026-09-16)
// ═══════════════════════════════════════════════════════════════════

/// One-way context switch: set SP to `sp` and branch to `pc`.
///
/// Never returns: the current Rust call chain and its stack are abandoned.
/// The caller must have released every lock and guard — nothing on the
/// abandoned stack will run again.
///
/// # Safety
/// `sp` must name a valid, exclusively-owned kernel stack and `pc` a valid
/// entry point for it.
#[unsafe(naked)]
unsafe extern "C" fn context_switch(sp: u64, pc: u64) -> ! {
    core::arch::naked_asm!("mov sp, x0", "br   x1",);
}

/// Resume a parked domain: SP must point at its saved exception frame.
///
/// Branches into the exception vectors' register-restore sequence, which
/// reloads `ELR_EL1`/`SPSR_EL1` and all GPRs from the frame and `eret`s back to
/// the parked caller's post-SVC instruction with the completion result
/// written into `gpr[0..2]`.
#[unsafe(naked)]
unsafe extern "C" fn resume_parked_context() -> ! {
    core::arch::naked_asm!("b __restore_context");
}

/// Park the current domain on `record` and switch to the next runnable
/// context. Never returns.
///
/// The current domain's exception frame stays parked at `frame_addr` on its
/// kernel stack; the domain resumes through [`resume_parked_context`] when
/// the record's terminal transition is delivered.
///
/// # Safety
/// Must be called from the SVC entry with the kernel lock released and no
/// access guards held; `frame_addr` must name the caller's saved exception
/// frame on the current kernel stack.
unsafe fn park_and_switch(frame_addr: u64, record: ObjectId) -> ! {
    let Some(nucleus_ptr) = nucleus_anchor() else {
        panic!("nucleus not booted by Kickstart")
    };
    // SAFETY: the anchor points at the live boot-carved Nucleus; the kernel
    // lock is released, so exclusive access is safe.
    let nucleus = unsafe { &mut *nucleus_ptr };

    let current = nucleus
        .current_domain
        .expect("blocked invocation without a current domain");
    {
        let Some(domain) = nucleus
            .pools
            .domains
            .get_live_mut(usize::try_from(current).expect("current domain index too wide"))
        else {
            panic!("current domain is not live");
        };
        domain.context = ExecutionContext::Parked { frame_addr, record };
    }

    // Pick the next runnable context. An empty queue means every domain is
    // blocked and no timer exists yet to wake anyone — the honest report is
    // a halt, not a fake return to the blocked caller.
    let next = nucleus
        .scheduler
        .pop()
        .expect("all domains blocked and no timer exists to wake anyone");
    nucleus.current_domain = Some(u32::from(next));

    let Some(domain) = nucleus.pools.domains.get_live_mut(usize::from(next)) else {
        panic!("runnable domain is not live");
    };
    let (sp, pc) = match domain.context {
        ExecutionContext::Parked { frame_addr, record } => {
            // Deliver the terminal outcome into the parked frame. A runnable
            // domain's record is always terminal: the wake enqueues the
            // domain only after the record's single terminal transition.
            let (x0, x1, x2) = match nucleus.pending.state(record) {
                Ok(PendingState::Completed { result0, result1 }) => {
                    // The invocation's handler reported `Blocked` at its SVC
                    // entry, so its success line belongs here: the resume is
                    // where the completed invocation's result is delivered.
                    let kind = match nucleus.pending.kind(record) {
                        Ok(PendingKind::NotificationWait) => "Notification::Wait",
                        Err(_) => "blocked invocation",
                    };
                    semi::println!("✅ {kind}(0x{result0:x}) resumed");
                    (syscall_status::SUCCESS, result0, result1)
                }
                // Cancellation outcomes need their D9 wire encoding; no
                // teardown path can produce them yet.
                Ok(PendingState::Cancelled) => {
                    panic!("cancelled record resumed before its D9 encoding exists")
                }
                Ok(PendingState::Waiting) => {
                    panic!("runnable domain's record has not reached its terminal transition")
                }
                Err(error) => {
                    panic!(
                        "runnable domain's record identity is stale: {:?}",
                        error.code()
                    )
                }
            };
            assert!(
                nucleus.pending.release(record).is_ok(),
                "failed to release a delivered record"
            );
            // SAFETY: the parked frame lives on the next domain's kernel
            // stack — valid, exclusively-owned memory that nothing executes
            // on while the domain is parked.
            let frame = unsafe { &mut *(frame_addr as *mut ExceptionContext) };
            frame.gpr[0] = x0;
            frame.gpr[1] = x1;
            frame.gpr[2] = x2;
            domain.context = ExecutionContext::Running;
            (frame_addr, resume_parked_context as *const () as u64)
        }
        ExecutionContext::NotStarted { pc, stack_top } => {
            domain.context = ExecutionContext::Running;
            (stack_top, pc)
        }
        ExecutionContext::Running => {
            panic!("runnable domain is already executing")
        }
    };

    semi::println!(
        "🔄 context switch: domain {current} parked, resuming domain {next} @ SP {sp:#x}, PC {pc:#x}"
    );
    // SAFETY: the target stack and entry point were validated above; the
    // current call chain is abandoned by design.
    unsafe { context_switch(sp, pc) }
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
