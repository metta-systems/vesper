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
    nucleus::objects::Nucleus,
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

//------------------------------------------------------------------------------
// Kernel entry point
//------------------------------------------------------------------------------

#[unsafe(no_mangle)]
extern "C" fn cap_invoke_handler(frame: &mut ExceptionContext) {
    let key = RawKey::from_wire(frame.gpr[0]);
    let op = frame.gpr[1];
    semi::println!(
        "CapInvoke SYSCALL(key: {key:?}, op: {op}) happened, we're at PC {:#016X}, SP {:#016X}, exception frame @ {:#016X}",
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
    let result = unsafe {
        #[allow(static_mut_refs)]
        KERNEL_LOCK.lock(|()| {
            let Some(ptr) = nucleus_anchor() else {
                panic!("nucleus not booted by Kickstart")
            };
            // SAFETY: the anchor points at the live boot-carved Nucleus.
            nucleus::api::handle_cap_invoke(unsafe { &mut *ptr }, key, op, &args)
        })
    };

    // let cap = current_domain().keytable.lookup(cap_slot)?;
    // let args = &[arg0, arg1, arg2, arg3, arg4, arg5]; // FIXME temp

    // let result = match cap.cap_type() {
    //     ObjectType::Untyped => api::untyped::invoke(cap, op, args), // retype, split
    //     ObjectType::Domain => api::domain::invoke(cap, op, args),   // activate, suspend...
    //     ObjectType::KeyTable => api::key_table::invoke(cap, op, args),
    //     ObjectType::Time => api::time::invoke(cap, op, args), // donate, split, merge
    //     ObjectType::Endpoint => api::endpoint::invoke(cap, op, args),
    //     ObjectType::Notification => api::notification::invoke(cap, op, args),
    //     ObjectType::EventCount => api::event_count::invoke(cap, op, args),
    //     ObjectType::None => Err(SyscallError::InvalidSlot),
    // };

    let (x0, x1, x2) = match result {
        Ok((v0, v1)) => (syscall_status::SUCCESS, v0, v1),
        Err(e) => e.code(),
    };
    // Return values
    semi::println!("CapInvoke SYSCALL(Return {x0:#x}, {x1:#x}, {x2:#x})",);
    // SAFETY: Not safe.
    unsafe {
        frame.gpr[0] = x0;
        frame.gpr[1] = x1;
        frame.gpr[2] = x2;
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
