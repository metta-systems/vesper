# SMP implementation plan

Companion to [smp-design.md](smp-design.md), which holds the
[decisions](smp-design.md#decisions) and the mechanism; open questions are only in
[smp-decisions.md](smp-decisions.md). The multicore scheduling tasks (per-core
Schedulers and Times, the cross-core Thread move, per-core deadline queues) were
done in [scheduling-implementation-plan.md](scheduling-implementation-plan.md). The
working rules, validation commands and `just` recipes of the
[main plan](capabilities-implementation-plan.md) apply unchanged.

This file holds only open work, in order. Each stage is verified before the next one
starts. Mark an item `- [x]` only after its validation has passed; a finished item
leaves a one-line marker in [smp-completed.md](smp-completed.md).

Done: P-SMP-0 (the decisions), P-SMP-1 (cores online and idle, on QEMU and the Pi 4
and Pi 5), P-SMP-2 (the kernel lock and per-core state), P-SMP-3 (inter-processor
interrupts) and P-SMP-4 (Threads on secondary cores); see
[smp-completed.md](smp-completed.md).

## P-SMP-6: the Pi 3 board run

- [ ] Board run of `smp-test` (`just boot-smp-test`, or `smp-test-<boards>.bin` on a card)
  on the Pi 3 (the Pi 4 and Pi 5 pass): `smp-test: PASS`, the first run of the BCM2836
  mailbox IPIs on a board, and of P-SMP-4's checks on a Cortex-A53.

## P-SMP-5: userspace on several cores

Needs: P-SMP-4. Nothing is pinned by default (SMP-19), so components run unpinned and
what breaks is fixed.

- [ ] The pin declaration (SMP-18): the allowed-core bitmask in `driver_threads!` entries and an
  `entry_cpu!`-style section for the first Thread, a manifest override in `image.toml`,
  read by Kickstart and handed to the dispatchers like `DispatchInit::priorities`.
- [ ] The root dispatchers place unpinned Threads across cores at run time (SMP-17).
- [ ] Run the production image and the board tests with everything unpinned; fix the
  multicore ordering bugs that show up (IO channels, EventCount rings, `log-server`, the
  audio ring, the compositor are the likely places), each with a test that reproduces it.

## Later

Future work; N-SMP-1 is in [smp-decisions.md](smp-decisions.md#future-work).

- [ ] N-SMP-2: finer kernel locking, as a measured pass.
- [ ] N-SMP-3: route each device line to the core of its receiver Thread
  (`route(line, cpu)` in `IrqChipOps`).
- [ ] N-SMP-4: asynchronous retirement of what runs on other cores, with quiescence
  detection.
