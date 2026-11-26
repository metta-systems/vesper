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

Done: SMP0 (the decisions), SMP1 except its board runs, SMP2 (the kernel lock and
per-core state), SMP3 (inter-processor interrupts; retirement and the cross-core
Reschedule moved to Phase 11 of the scheduling plan); see
[smp-completed.md](smp-completed.md).

## SMP1: cores online, idle

Secondaries run no Threads; they enter the kernel only to prepare themselves at the
interrupt controller and to take IPIs. Order: QEMU `raspi3b` and `raspi4b`, then the
Pi 4, Pi 3 and Pi 5 boards.

- [ ] Board runs of `smp-test` (`just boot-smp-test`, or `smp-test-<boards>.bin` on a card)
  on the Pi 3 and Pi 5 (the Pi 4 passes): `smp-test: PASS`, which checks `SMPEN`
  coherency with the cross-core counter and an IPI round trip with each core; the Pi 3 is
  the first run of the BCM2836 mailbox IPIs, the Pi 5 the first of the PSCI component and
  the `no-map` reservation.

## SMP4: Threads on secondary cores

The scheduling work is Phase 11 of
[scheduling-implementation-plan.md](scheduling-implementation-plan.md). Prerequisites:
SMP3.

## SMP5: userspace on several cores

Needs: SMP4. Nothing is pinned by default, so components run unpinned and what breaks is
fixed.

- [ ] The pin declaration: a core bitmask in `driver_threads!` entries and an
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
