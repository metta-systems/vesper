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
    crate::objects::{Nucleus, ObjectPool, arch::ArchPools, domain::DcbPages},
    cfg_if::cfg_if,
    core::{
        arch::asm,
        cell::{LazyCell, UnsafeCell},
        panic::PanicInfo,
        time::Duration,
    },
    libcpu::endless_sleep,
    libexception::arch::aarch64::ExceptionContext,
    liblocking::{IRQSafeNullLock, interface::Mutex},
    liblog::{info, println, warn},
    libmemory::mmu::AccessPermissions,
    libobject::{ArchType, CapError, KeySlot},
    libqemu::{semi_print, semi_println},
};

/// Syscall API - capability invocation handlers
mod api;
/// Nucleus objects implementations
mod objects;

// TODO: Split this into read-only part, that does not need locks, per-cpu mutable part that does not need locks,
// TODO: Shared atomic counters that do not need locks and shared mutable collections that DO need locks (but should be minority)
/// Global kernel state, protected by The Great Kernel Lock
static mut NUCLEUS: IRQSafeNullLock<LazyCell<Nucleus<objects::ArchObjectsImpl>>> =
    IRQSafeNullLock::new(LazyCell::new(|| {
        let mut n = Nucleus::<objects::ArchObjectsImpl> {
            current_domain: None,
            dcb_pages: DcbPages::new(),
            pools: objects::nucleus::NucleusPools {
                domains: unsafe { ObjectPool::new(0x1000 as *mut u8, 16384) }, // TODO: proper alloc...
                arch: unsafe { ArchPools::new() },
            },
        };
        n.create_domain();
        n
    }));

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
        {}",
        exc
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
    cap_invoke_handler(e)
}

#[cfg(any(test, feature = "test_build"))]
#[unsafe(no_mangle)]
extern "C" fn current_elx_synchronous(e: &mut ExceptionContext) {
    {
        const TEST_SVC_ID: u64 = 0x1337;

        let esr_el1 = esr_el1::EsrEL1(LocalRegisterCopy::new(ESR_EL1.get()));

        if let Some(ESR_EL1::EC::Value::SVC64) = esr_el1.exception_class()
            && esr_el1.iss() == TEST_SVC_ID
        {
            liblog::println!("Serving syscall {TEST_SVC_ID}");
            return;
        }
    }

    if debug::exception_dump(e) {
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
    cap_invoke_handler(e)
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
fn cap_invoke_handler(frame: &mut ExceptionContext) {
    let cap_slot = frame.gpr[0] as u32;
    let op = frame.gpr[1] as u32;
    semi_println!(
        "CapInvoke SYSCALL(cap: {cap_slot}, op: {op}) happened, we're at PC {:#016X}, SP {:#016X}, exception frame @ {:#016X}",
        get_pc(),
        get_sp(),
        frame as *mut _ as u64,
    );

    // semi_println!("{}", frame);

    let args: &[u64; 6] = &frame.gpr[2..=7].try_into().unwrap();

    let result = unsafe {
        #[allow(static_mut_refs)]
        NUCLEUS.lock(|nucleus| api::handle_cap_invoke(nucleus, cap_slot, op, args))
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
    //     ObjectType::Buffer => api::buffer::invoke(cap, op, args), // map, unmap, query
    //     ObjectType::None => Err(SyscallError::InvalidSlot),
    // };

    let (x0, x1, x2) = match result {
        Ok((v0, v1)) => (0, v0, v1),
        Err(e) => e.code(),
    };
    // Return values
    semi_println!("CapInvoke SYSCALL(Return {x0:#x}, {x1:#x}, {x2:#x})",);
    unsafe {
        frame.gpr[0] = x0;
        frame.gpr[1] = x1;
        frame.gpr[2] = x2;
    }
}

fn get_pc() -> u64 {
    let pc: u64;
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
