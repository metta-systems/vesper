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

- [ ] Kickstart: reserve `/reserved-memory` `no-map` nodes (Pi 5 `atf@0`) as it does
  `/memreserve/`, if the firmware's memory node does not already exclude them; check that
  chainboot leaves the first page alone.
- [ ] Kickstart: carve a trap stack and a per-core record per core from accounted memory,
  sized from `/cpus`, and map the stacks in the kernel half next to the boot core's. The
  record holds the core's logical index (its `/cpus` order) and `CoreAffinity`; the kernel
  reads its index from there, not from `MPIDR_EL1`.
- [ ] A secondary entry: the EL2 prologue factored out of `_startup_in_rust` (and the
  `qemu` EL3 → EL2 step), then `el_switch` with the shared `TTBR1`/`TCR`/`MAIR`/`VBAR`,
  its own `SP_EL1` and `TPIDR_EL1`, no BSS clear, no atomics before its MMU is on.
- [ ] Release the cores just before Kickstart's own drop to EL1 through the loaded
  component (`cpu_start::LoadedCpuStart::start`), with the mixed-attribute handshake of
  the design (plain stores cleaned to PoC; "online"
  signalled by a cacheable atomic after the MMU is on).
- [ ] Nucleus: a secondary idle loop (`wfi`, interrupts masked, no Thread) and the
  `MultiCoreMain` state transition in `libkernel-state`.
- [ ] Test kernel `smp-test` (`just _test-smp`, `-rpi4`): every core reports online with
  its MPIDR and logical index; the boot core waits with a timeout. Board runs on the Pi 4,
  Pi 3 and Pi 5, checking `SMPEN` coherency with a cross-core cacheable counter.

## SMP2: one kernel lock and per-core state

- [ ] Move the per-core fields of `Nucleus` and the per-core statics (`IDLING`,
  `timer::PROGRAMMED`, the `fp_unit` owner) into the per-core record, reached through
  `TPIDR_EL1`. The nucleus totals become per-core and each core publishes its own in its
  root Scheduler record (C11(a)).
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
