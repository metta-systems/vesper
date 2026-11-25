# Scheduling implementation plan

The checklist for Time and userspace scheduling; no scheduling task lives anywhere else. The rules are in [`scheduling-design.md`](scheduling-design.md#decisions), open questions only in [`scheduling-decisions.md`](scheduling-decisions.md). The working rules, validation commands and `just` recipes of the [main plan](capabilities-implementation-plan.md) apply unchanged.

Mark an item `- [x]` only after implementation and validation have passed. A finished item leaves a one-line marker in [`scheduling-completed.md`](scheduling-completed.md).

Phases 1 to 8 and 10 are finished, as are the hierarchy items of Phase 9: see [`scheduling-completed.md`](scheduling-completed.md).

## Phase 9 — Hierarchy and domains (what is left)

The decisions and finished items are in [`scheduling-completed.md`](scheduling-completed.md#phase-9--hierarchy-and-domains-done-items).

- [ ] Client ownership APIs that preserve budget on pre-commit errors without relying on destructor side effects (moved here from Phase 8, maintainer decision 2026-10-18, since donation and the strict scheduler tree decide how budget is owned). Candidate shape: an explicit `Loan` handle with `commit` and `refund`, no `Drop`.
- [ ] Temporal service contracts for compositional real-time guarantees, if the project needs them.

**Exit:** storage cannot mint execution authority; delegated time is spent at most once; execution, observation and ownership agree across layers; the nucleus contains no scheduling policy.

## Phase 10 — Retire the kernel queue (done)

Finished: see [`scheduling-completed.md`](scheduling-completed.md#phase-10--retire-the-kernel-queue).

## Root dispatcher priorities (done)

Thread priority in the root dispatcher, the input level between normal and high, and the starvation guard (maintainer decisions, 2026-10-29; the rule is in the [scheduling design](scheduling-design.md#decisions)). Finished: see [`scheduling-completed.md`](scheduling-completed.md#root-dispatcher-priorities).

## Phase 11 — Multicore

The scheduling part of SMP4 in [`smp-implementation-plan.md`](smp-implementation-plan.md); it starts after SMP3 there (inter-processor interrupts). The rules are in the [SMP design's decisions](smp-design.md#decisions) (C4, C4.1–C4.3, C5, C7, C9).

- [ ] Multicore Times: a Time belongs to one core; Transfer, Merge and dispatch reject another core's Time.
- [ ] A root Scheduler, root dispatcher and boot Time per core, created by Kickstart; a Scheduler's core field set once (Kickstart for the roots, `Adopt` copies the parent's).
- [ ] `Scheduler.Dispatch` `x6` checked against the Scheduler's core; the Thread record's `cpu` reports the real core.
- [ ] Each core's one-shot timer for its dispatch limit and budget, and a per-core deadline queue for the wait timeouts and `ArmTimer` deadlines of its Schedulers.
- [ ] The cross-core Thread move: the Thread's bound Time must be empty, and the Thread is rebound to a Time of the destination core in the same call. `Scheduler.MoveThread` also moves between Schedulers of different cores, taking the destination Time key, with `WRITE` on both Schedulers, the Thread and that Time.
- [ ] A per-core root dispatcher in `userspace/dispatcher`, placing unpinned Threads at run time.
- [ ] Tests: two Threads on two cores in parallel with exact per-core accounting; a cross-core Signal waking a waiter; Transfer to another core's Time rejected; a cross-core move; the existing preempt, sync and ppc suites run on core 1.

Still parked ([`scheduling-decisions.md`](scheduling-decisions.md), not needed by per-core dispatchers):

- [ ] `Scheduler.replace`: a dispatcher replaces a Thread running on another CPU.
- [ ] Non-blocking dispatch, and the activation invariant of one outstanding request per dispatcher across cores, including a dispatcher already running on another core.
- [ ] Batched remote dispatch.
