#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

use {
    core::mem::{align_of, offset_of, size_of},
    vesper_exceptions::{
        PrivilegeLevel,
        arch::{ExceptionContext, ExceptionOrigin, SavedContext},
        current_privilege_level,
    },
};

#[path = "../../../tests/common/mod.rs"]
mod common;

/// libmachine unit tests must execute in kernel mode.
#[test_case]
fn test_runner_executes_in_kernel_mode() {
    let (level, _) = current_privilege_level();

    assert!(level == PrivilegeLevel::Kernel)
}

#[test_case]
fn exception_context_layout_matches_vector_frame() {
    assert_eq!(size_of::<ExceptionContext>(), 288);
    assert_eq!(align_of::<ExceptionContext>(), 16);
    assert_eq!(offset_of!(ExceptionContext, gpr), 0);
    assert_eq!(offset_of!(ExceptionContext, lr), 240);
    assert_eq!(offset_of!(ExceptionContext, spsr_el1), 248);
    assert_eq!(offset_of!(ExceptionContext, elr_el1), 256);
    assert_eq!(offset_of!(ExceptionContext, sp), 264);
    assert_eq!(offset_of!(ExceptionContext, origin), 272);
    assert_eq!(offset_of!(ExceptionContext, tpidr_el0), 280);
    assert_eq!(offset_of!(SavedContext, tpidr_el0), 280);
    assert_eq!(size_of::<SavedContext>(), 288);
    assert_eq!(align_of::<SavedContext>(), 8);
}

#[test_case]
fn exception_origin_has_explicit_vector_group_values() {
    assert_eq!(size_of::<ExceptionOrigin>(), 8);
    assert_eq!(ExceptionOrigin::CurrentSp0 as u64, 0);
    assert_eq!(ExceptionOrigin::CurrentSpx as u64, 1);
    assert_eq!(ExceptionOrigin::LowerAarch64 as u64, 2);
    assert_eq!(ExceptionOrigin::LowerAarch32 as u64, 3);
}

#[test_case]
fn saved_context_el1t_initializes_masked_execution_state() {
    const PC: u64 = 0x0000_0000_8000_1234;
    const SP: u64 = 0xffff_0000_8100_0000;
    const SAVED: SavedContext = SavedContext::el1t(PC, SP);

    assert_eq!(SAVED.gpr, [0; 30]);
    assert_eq!(SAVED.lr, 0);
    assert_eq!(SAVED.spsr_el1, 0x3c4);
    assert_eq!(SAVED.spsr_el1 & 0x3c0, 0x3c0);
    assert_eq!(SAVED.spsr_el1 & 0xf, 0x4);
    assert_eq!(SAVED.elr_el1, PC);
    assert_eq!(SAVED.sp, SP);
    assert_eq!(SAVED.origin, ExceptionOrigin::CurrentSp0);
    assert_eq!(SAVED.tpidr_el0, 0);
    assert_eq!(ExceptionContext::from(SAVED).save(), SAVED);
}

/// `SPSR_EL1.I`: the IRQ mask bit.
const SPSR_IRQ_MASK: u64 = 1 << 7;

#[test_case]
fn saved_context_el0_takes_irqs_with_debug_serror_and_fiq_masked() {
    const PC: u64 = 0x40_0000;
    const SP: u64 = 0x80_0000;
    const SAVED: SavedContext = SavedContext::el0(PC, SP, 0x1234);

    assert_eq!(SAVED.spsr_el1, 0x340);
    assert_eq!(SAVED.spsr_el1 & SPSR_IRQ_MASK, 0, "EL0 must be preemptible");
    assert_eq!(SAVED.spsr_el1 & 0xf, 0x0, "EL0t");
    assert_eq!(SAVED.gpr[0], 0x1234);
    assert_eq!(SAVED.origin, ExceptionOrigin::LowerAarch64);
}

#[test_case]
fn saved_context_el1t_interruptible_differs_only_in_the_irq_mask() {
    const PC: u64 = 0x8_1000;
    const SP: u64 = 0x8_2000;
    const MASKED: SavedContext = SavedContext::el1t(PC, SP);
    const INTERRUPTIBLE: SavedContext = SavedContext::el1t_interruptible(PC, SP);

    assert_eq!(INTERRUPTIBLE.spsr_el1, 0x344);
    assert_eq!(INTERRUPTIBLE.spsr_el1 | SPSR_IRQ_MASK, MASKED.spsr_el1);
    assert_eq!(INTERRUPTIBLE.origin, ExceptionOrigin::CurrentSp0);
    assert_eq!(INTERRUPTIBLE.elr_el1, PC);
    assert_eq!(INTERRUPTIBLE.sp, SP);
}

#[test_case]
fn saved_context_round_trips_registers_status_sp_and_origin() {
    let origins = [
        ExceptionOrigin::CurrentSp0,
        ExceptionOrigin::CurrentSpx,
        ExceptionOrigin::LowerAarch64,
        ExceptionOrigin::LowerAarch32,
    ];
    // Raw-copy tests also cover reserved status bits; these values never reach eret.
    let statuses = [0, 0x3c4, 0x3c5, 0xf000_03c0, u64::MAX];

    for origin in origins {
        for spsr_el1 in statuses {
            let mut expected = SavedContext {
                gpr: [0; 30],
                lr: 0xffff_0000_1234_5678,
                spsr_el1,
                elr_el1: 0x0000_0000_8123_4560,
                sp: 0xffff_0000_8765_4320,
                origin,
                tpidr_el0: 0x7150_0000_0000_0000 | spsr_el1,
            };
            for (index, register) in expected.gpr.iter_mut().enumerate() {
                *register = 0xa5a5_5a5a_0000_0000 | u64::try_from(index).unwrap();
            }

            let mut frame = ExceptionContext::from(SavedContext::el1t(0, 0));
            frame.restore(expected);
            assert_eq!(frame.gpr, expected.gpr);
            assert_eq!(frame.gpr[9], 0xa5a5_5a5a_0000_0009);
            assert_eq!(frame.lr, expected.lr);
            assert_eq!(frame.spsr_el1.raw(), spsr_el1);
            assert_eq!(frame.elr_el1, expected.elr_el1);
            assert_eq!(frame.sp, expected.sp);
            assert_eq!(frame.origin, origin);
            assert_eq!(frame.tpidr_el0, expected.tpidr_el0);
            assert_eq!(frame.save(), expected);

            let mut second_frame = ExceptionContext::from(SavedContext::el1t(4, 16));
            second_frame.restore(frame.save());
            assert_eq!(second_frame.save(), expected);
        }
    }
}

#[test_case]
fn saved_context_does_not_retain_transient_frame_storage() {
    let mut expected = SavedContext::el1t(0x8123_4560, 0xffff_0000_8765_4320);
    expected.gpr[9] = 0x0123_4567_89ab_cdef;
    expected.lr = 0xffff_0000_1234_5678;
    expected.spsr_el1 = 0xf000_03c5;
    expected.origin = ExceptionOrigin::CurrentSpx;
    let mut frame = ExceptionContext::from(expected);
    let saved = frame.save();

    frame.restore(SavedContext::el1t(0x9000_0000, 0xffff_0000_9000_0000));
    assert_ne!(frame.save(), saved);
    assert_eq!(saved, expected);

    // SavedContext is Copy: using it for another frame does not consume the continuation.
    frame.restore(saved);
    assert_eq!(frame.save(), saved);
}
