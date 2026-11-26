#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! smp-test: the secondary cores come online. Kickstart releases them through the
//! start-method component the device tree names; each reports, at EL1 with its MMU and
//! caches on, the affinity it reads from its own `MPIDR_EL1` under its logical index,
//! and every core then adds to one cacheable counter at the same time, which loses no
//! update only when the cores' caches are coherent.
//!
//! Runs on the real Kickstart boot path. Under QEMU (`just _test-smp`, `_test-smp-rpi4`)
//! the semihosting exit status is the result; on a board (`just boot-smp-test`) the
//! console shows `smp-test: PASS` or `smp-test: FAIL`.

use {
    kickstart::{early_println, kickstart_init_el2, smp},
    // `libkicktest` supplies the panic handler, so it is linked in every build.
    libkicktest as _,
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

/// Print `what` with its outcome; whether it holds.
fn check(holds: bool, what: core::fmt::Arguments) -> bool {
    early_println!("smp-test: {} {what}", if holds { "ok  " } else { "FAIL" });
    holds
}

/// The logical indices whose bits `mask` sets.
fn indices(mask: u64) -> impl Iterator<Item = usize> {
    (0..64).filter(move |index| mask & (1 << index) != 0)
}

pub fn run() -> ! {
    let boot = nucleus::core_record::current();
    early_println!(
        "smp-test: boot core {} (affinity {:#x})",
        boot.index,
        boot.affinity
    );
    let released = smp::released_cores();
    // `el1_entry` already waited for the released cores.
    let online = smp::online_cores();
    let mut passed = true;

    passed &= check(
        online == released,
        format_args!(
            "every released core is online: released {released:#06b}, online {online:#06b}"
        ),
    );
    // QEMU's raspi3b and raspi4b model four cores, the boot core first.
    #[cfg(feature = "qemu")]
    {
        passed &= check(
            released == 0b1110,
            format_args!("QEMU releases cores 1 to 3: {released:#06b}"),
        );
    }

    for index in indices(online) {
        let affinity = smp::reported_affinity(index);
        early_println!("smp-test: core {index} online, affinity {affinity:#x}");
        let shared = affinity == boot.affinity
            || indices(online)
                .any(|other| other != index && smp::reported_affinity(other) == affinity);
        passed &= check(!shared, format_args!("core {index}'s affinity is its own"));
    }

    let (done, counter) = smp::coherency_check(online);
    let expected = smp::COHERENCY_ROUNDS * (u64::from(online.count_ones()) + 1);
    passed &= check(
        done == online,
        format_args!("every online core finished the coherency round: {done:#06b}"),
    );
    passed &= check(
        counter == expected,
        format_args!("the shared counter lost no update: {counter} of {expected}"),
    );

    if passed {
        early_println!("smp-test: PASS");
    } else {
        early_println!("smp-test: FAIL");
    }
    finish(passed)
}

#[cfg(feature = "qemu")]
fn finish(passed: bool) -> ! {
    if passed {
        libqemu::semihosting::exit_success()
    } else {
        libqemu::semihosting::exit_failure()
    }
}

#[cfg(not(feature = "qemu"))]
fn finish(_passed: bool) -> ! {
    libcpu::endless_sleep()
}
