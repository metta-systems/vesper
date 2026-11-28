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

The scheduling part of P-SMP-4 in [`smp-implementation-plan.md`](smp-implementation-plan.md); it started after P-SMP-3 there (inter-processor interrupts). The rules are in the [SMP design's decisions](smp-design.md#decisions) (SMP-6 to SMP-12, SMP-17 to SMP-20).

Done: per-core roots, synchronous retirement across cores, the cross-core Thread move and the suites on core 1 (see [`scheduling-completed.md`](scheduling-completed.md#phase-11--multicore)). The root dispatcher still hands every managed Thread to the boot core; placing them across cores is P-SMP-5 in [`smp-implementation-plan.md`](smp-implementation-plan.md).

Still parked ([`scheduling-decisions.md`](scheduling-decisions.md), not needed by per-core dispatchers):

- [ ] `Scheduler.replace`: a dispatcher replaces a Thread running on another CPU.
- [ ] Non-blocking dispatch, and the activation invariant of one outstanding request per dispatcher across cores, including a dispatcher already running on another core.
- [ ] Batched remote dispatch.
