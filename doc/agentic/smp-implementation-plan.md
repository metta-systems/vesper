# SMP implementation plan

Companion to [smp-design.md](smp-design.md), which holds the
[decisions](smp-design.md#decisions) and the mechanism; open questions are only in
[smp-decisions.md](smp-decisions.md). Multicore scheduling
tasks (per-core Schedulers and Times, the cross-core Thread move, per-core deadline
queues) live in Phase 11 of
[scheduling-implementation-plan.md](scheduling-implementation-plan.md). The working
rules, validation commands and `just` recipes of the
[main plan](capabilities-implementation-plan.md) apply unchanged.

This file holds only open work, in order. Each stage is verified before the next one
starts. Mark an item `- [x]` only after its validation has passed; a finished item
leaves a one-line marker in [smp-completed.md](smp-completed.md).

Done: SMP0 (the decisions); see [smp-completed.md](smp-completed.md).

## SMP1: cores online, idle

Secondaries run no Threads; the kernel lock is still the null lock because they never
enter the shared kernel state. Order: QEMU `raspi3b` and `raspi4b`, then the Pi 4, Pi 3
and Pi 5 boards.

- [ ] Board runs of `smp-test` (`just boot-smp-test`, or `smp-test-<boards>.bin` on a card)
  on the Pi 4, Pi 3 and Pi 5: `smp-test: PASS`, which checks `SMPEN` coherency with the
  cross-core counter; the Pi 5 is the first run of the PSCI component and of the
  `no-map` reservation.

## SMP2: one kernel lock and per-core state

- [ ] Replace `KERNEL_LOCK`'s `IRQSafeNullLock` with a ticket spinlock taken on every
  kernel entry; host model tests of the lock, and a debug check that no path waits for
  another core while holding it.
- [ ] Release the lock around the idle `wfi`, and re-take it in the idle-path interrupt.
- [ ] Payload publication with Acquire/Release (contract, Notification and EventCount):
  audit Signal/Advance and record publication for the orderings the lock does not give
  to userspace readers on another core (scheduler records' sequence lock).
- [ ] Panic: park the other cores through a flag they poll in idle until SMP3; console and
  semihosting lines under the lock.
- [ ] All existing e2e tests pass unchanged with the secondaries online and idle.

## SMP3: inter-processor interrupts

- [ ] `IrqChipOps`: `init_cpu` and `send_ipi`, edited in place. BCM2836: per-core timer
  and mailbox 0 registers (checked against QA7); GIC-400: banked CPU interface, PPI
  enable, `GICD_SGIR`. Host tests in each component's typed register views.
- [ ] Nucleus: IPI kinds (Reschedule, Stop, Halt) as atomic bits in the per-core record,
  one hardware line or SGI.
- [ ] Device lines stay on the boot core; a Notification signal or EventCount advance
  whose waiter's core is another sends Reschedule.
- [ ] Synchronous retirement of a Thread running on another core and of an AddressSpace
  current on other cores: Stop IPIs, the kernel lock released until every core
  acknowledged, an "active on" core mask in the AddressSpace before its ASID is reused.
- [ ] Panic sends Halt.
- [ ] `smp-test` cases: IPI round trip on each core, retirement of a Thread running on
  another core, AddressSpace retirement while current on two cores.

## SMP4: Threads on secondary cores

The scheduling work is Phase 11 of
[scheduling-implementation-plan.md](scheduling-implementation-plan.md). Prerequisites:
SMP3.

## SMP5: userspace on several cores

Needs: SMP4. Nothing is pinned by default, so components run unpinned and what breaks is
fixed.

- [ ] The pin declaration: a core byte in `driver_threads!` entries and an
  `entry_cpu!`-style section for the first Thread, a manifest override in `image.toml`,
  read by Kickstart and handed to the dispatchers like `DispatchInit::priorities`.
- [ ] The root dispatchers place unpinned Threads across cores at run time.
- [ ] Run the production image and the board tests with everything unpinned; fix the
  multicore ordering bugs that show up (IO channels, EventCount rings, `log-server`, the
  audio ring, the compositor are the likely places), each with a test that reproduces it.

## Later

- [ ] Finer kernel locking, as a measured pass.
- [ ] Route each device line to the core of its receiver Thread (`route(line, cpu)` in
  `IrqChipOps`).
- [ ] Asynchronous retirement of what runs on other cores, with quiescence detection.
