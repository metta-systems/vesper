#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! smp-test: the secondary cores come online. Kickstart releases them through the
//! start-method component the device tree names; each reports, at EL1 with its MMU and
//! caches on, the affinity it reads from its own `MPIDR_EL1` under its logical index,
//! and every core then adds to one cacheable counter at the same time, which loses no
//! update only when the cores' caches are coherent. The test then starts the nucleus,
//! whose interrupt controller each secondary prepares itself at, and makes an IPI round
//! trip with each: the nucleus pings the core (`IPI_PING`, through its test-only ping
//! SVC, built with the nucleus's `smp_test` feature), the core answers the boot core
//! (`IPI_PONG`), and the test sees both take their IPI in the per-core records.
//!
//! Runs on the real Kickstart boot path. Under QEMU (`just _test-smp`, `_test-smp-rpi4`)
//! the semihosting exit status is the result; on a board (`just boot-smp-test`) the
//! console shows `smp-test: PASS` or `smp-test: FAIL`.

use {
    core::sync::atomic::Ordering,
    kickstart::{early_println, image::BootObjects, kickstart_init_el2, smp},
    // `libkicktest` supplies the panic handler, so it is linked in every build.
    libkicktest as _,
    nucleus::core_record::{self, SMP_TEST_PING_SVC},
};

/// How long each step of a round trip may take, in milliseconds.
const ROUND_TRIP_TIMEOUT_MS: u64 = 1000;

/// Wait until `done` holds, calling `step` between looks, for at most
/// [`ROUND_TRIP_TIMEOUT_MS`]; whether it held.
fn wait_for(done: impl Fn() -> bool, step: impl Fn()) -> bool {
    let ticks = nucleus::timer::frequency() * ROUND_TRIP_TIMEOUT_MS / 1000;
    let start = nucleus::timer::counter();
    while !done() {
        if nucleus::timer::counter().saturating_sub(start) > ticks {
            return false;
        }
        step();
    }
    true
}

/// Ask the nucleus to ping logical CPU `cpu`; whether the controller sent it.
fn ping(cpu: usize) -> bool {
    let refused: u64;
    // SAFETY: the nucleus's ping SVC reads and writes `x0` only.
    unsafe {
        core::arch::asm!(
            "svc #{svc}",
            svc = const SMP_TEST_PING_SVC,
            inout("x0") cpu as u64 => refused,
        );
    }
    refused == 0
}

/// Let this core take pending interrupts for a moment: the boot Thread runs with them
/// masked, and its IPI line is an interrupt like any other.
fn take_interrupts() {
    // SAFETY: the boot Thread runs at EL1; an interrupt taken here is an ordinary kernel
    // entry, and the mask is back before anything else runs.
    unsafe {
        core::arch::asm!(
            "msr daifclr, #2",
            "isb",
            "msr daifset, #2",
            options(nostack)
        );
    }
}

/// An IPI round trip with logical CPU `cpu`: ping it, see it take the ping, then take
/// its answer on this core. The ping is refused until the core has prepared itself at
/// the interrupt controller, so it is sent again until accepted.
fn round_trip(cpu: usize) -> Result<(), &'static str> {
    let target = core_record::of(cpu).ok_or("no such core")?;
    let own = core_record::current();
    let (target_taken, own_taken) = (
        target.ipis_taken.load(Ordering::Acquire),
        own.ipis_taken.load(Ordering::Acquire),
    );
    if !wait_for(|| ping(cpu), core::hint::spin_loop) {
        return Err("the ping was never accepted");
    }
    if !wait_for(
        || target.ipis_taken.load(Ordering::Acquire) > target_taken,
        core::hint::spin_loop,
    ) {
        return Err("the core never took the ping");
    }
    if !wait_for(
        || own.ipis_taken.load(Ordering::Acquire) > own_taken,
        take_interrupts,
    ) {
        return Err("the boot core never took the answer");
    }
    Ok(())
}

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

    // The nucleus with its interrupt controller: the secondaries waiting in its idle loop
    // prepare themselves at the controller once it is installed.
    let _boot = BootObjects::bootstrap();
    for index in indices(online) {
        let result = round_trip(index);
        passed &= check(
            result.is_ok(),
            format_args!("IPI round trip with core {index}: {result:?}"),
        );
    }

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
    }
    libqemu::semihosting::exit_failure()
}

#[cfg(not(feature = "qemu"))]
fn finish(_passed: bool) -> ! {
    libcpu::endless_sleep()
}
