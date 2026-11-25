# Scheduling implementation plan

The checklist for Time and userspace scheduling. [`capabilities-implementation-plan.md`](capabilities-implementation-plan.md) points here for these items; no scheduling task lives in both files. The design authority is the [Time contract](capabilities-contract.md#time-and-userspace-scheduling). Inputs: [`capability-scheduler-design.md`](capability-scheduler-design.md) (design discussion), [`capabilities-scheduling.md`](capabilities-scheduling.md) (survey of existing systems), [`capabilities-time.md`](capabilities-time.md) (Composite comparison). Open questions are only in [`capabilities-decisions.md`](capabilities-decisions.md) (D5, D8). The working rules, validation commands and `just` recipes of the main plan apply unchanged.

Mark an item `- [x]` only after implementation and validation have passed. A finished item leaves a one-line marker in [`capabilities-completed.md`](capabilities-completed.md), and moves to [`scheduling-completed.md`](scheduling-completed.md).

**Model in one paragraph.** Time is a transferable, accounted budget with no priority. The nucleus charges the running Time, enforces limits with a one-shot timer and reports block, expiry and wakeup events to the responsible scheduler. A Thread whose Time is exhausted is ineligible to run until more budget is Transferred in. Every policy (order, priorities, renewal, fairness) is userspace scheduler code.

Phases 1 to 8 and 10 are finished, as are the hierarchy items of Phase 9: see [`scheduling-completed.md`](scheduling-completed.md).

## Phase 9 — Hierarchy and domains (what is left)

The decisions and finished items are in [`scheduling-completed.md`](scheduling-completed.md#phase-9--hierarchy-and-domains-done-items).

- [ ] Client ownership APIs that preserve budget on pre-commit errors without relying on destructor side effects (moved here from Phase 8, maintainer decision 2026-10-18, since donation and the strict scheduler tree decide how budget is owned). Candidate shape: an explicit `Loan` handle with `commit` and `refund`, no `Drop`.
- [ ] Temporal service contracts for compositional real-time guarantees, if the project needs them.

**Exit:** storage cannot mint execution authority; delegated time is spent at most once; execution, observation and ownership agree across layers; the nucleus contains no scheduling policy.

## Phase 10 — Retire the kernel queue (done)

Finished: see [`scheduling-completed.md`](scheduling-completed.md#phase-10--retire-the-kernel-queue).

## Root dispatcher priorities (done)

Thread priority in the root dispatcher, the input level between normal and high, and the starvation guard (maintainer decisions, 2026-10-29; the rule is in the [contract](capabilities-contract.md#time-and-userspace-scheduling)). Finished: see [`scheduling-completed.md`](scheduling-completed.md#root-dispatcher-priorities).

## Phase 11 — Multicore

The scheduling part of SMP4 in [`smp-implementation-plan.md`](smp-implementation-plan.md); it starts after SMP3 there (inter-processor interrupts). The rules are in the [SMP design's decisions](smp-design.md#decisions) (C4, C4.1–C4.3, C5, C7, C9).

- [ ] Multicore Times: a Time belongs to one core; Transfer, Merge and dispatch reject another core's Time.
- [ ] A root Scheduler, root dispatcher and boot Time per core, created by Kickstart; a Scheduler's core field set once (Kickstart for the roots, `Adopt` copies the parent's).
- [ ] `Scheduler.Dispatch` `x6` checked against the Scheduler's core; the Thread record's `cpu` reports the real core.
- [ ] Each core's one-shot timer for its dispatch limit and budget, and a per-core deadline queue for the wait timeouts and `ArmTimer` deadlines of its Schedulers.
- [ ] The cross-core Thread move: the Thread's bound Time must be empty, and the Thread is rebound to a Time of the destination core in the same call. `Scheduler.MoveThread` also moves between Schedulers of different cores, taking the destination Time key, with `WRITE` on both Schedulers, the Thread and that Time.
- [ ] A per-core root dispatcher in `userspace/dispatcher`, placing unpinned Threads at run time.
- [ ] Tests: two Threads on two cores in parallel with exact per-core accounting; a cross-core Signal waking a waiter; Transfer to another core's Time rejected; a cross-core move; the existing preempt, sync and ppc suites run on core 1.

Still parked (D5/D8, not needed by per-core dispatchers):

- [ ] `Scheduler.replace`: a dispatcher replaces a Thread running on another CPU.
- [ ] Non-blocking dispatch, and the activation invariant of one outstanding request per dispatcher across cores, including a dispatcher already running on another core.
- [ ] Batched remote dispatch.
