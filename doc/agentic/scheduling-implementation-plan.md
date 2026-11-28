# Scheduling implementation plan

The checklist for Time and userspace scheduling; no scheduling task lives anywhere else. The rules are in [`scheduling-design.md`](scheduling-design.md#decisions), open questions only in [`scheduling-decisions.md`](scheduling-decisions.md). The working rules, validation commands and `just` recipes of the [main plan](capabilities-implementation-plan.md) apply unchanged.

Mark an item `- [x]` only after implementation and validation have passed. A finished item leaves a one-line marker in [`scheduling-completed.md`](scheduling-completed.md).

Done: P-SCHEDULING-1 to P-SCHEDULING-12 (P-SCHEDULING-9 and P-SCHEDULING-11 with an open rest below); see [`scheduling-completed.md`](scheduling-completed.md).

## P-SCHEDULING-13: hierarchy and domains, the rest

The rest of P-SCHEDULING-9.

- [ ] Client ownership APIs that preserve budget on pre-commit errors without relying on destructor side effects (moved here from P-SCHEDULING-8, maintainer decision 2026-10-18, since donation and the strict scheduler tree decide how budget is owned). Candidate shape: an explicit `Loan` handle with `commit` and `refund`, no `Drop`.
- [ ] Temporal service contracts for compositional real-time guarantees, if the project needs them.

**Exit:** storage cannot mint execution authority; delegated time is spent at most once; execution, observation and ownership agree across layers; the nucleus contains no scheduling policy.

## P-SCHEDULING-14: multicore, parked

The rest of P-SCHEDULING-11, the scheduling part of P-SMP-4 in [`smp-implementation-plan.md`](smp-implementation-plan.md). The rules are in the [SMP design's decisions](smp-design.md#decisions) (SMP-6 to SMP-12, SMP-17 to SMP-20). The root dispatcher still hands every managed Thread to the boot core; placing them across cores is P-SMP-5.

Parked (Q-SCHEDULING-4, not needed by per-core dispatchers):

- [ ] `Scheduler.replace`: a dispatcher replaces a Thread running on another CPU.
- [ ] Non-blocking dispatch, and the activation invariant of one outstanding request per dispatcher across cores, including a dispatcher already running on another core.
- [ ] Batched remote dispatch.
