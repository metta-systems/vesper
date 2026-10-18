# Scheduling implementation plan

The checklist for Time and userspace scheduling. [`capabilities-implementation-plan.md`](capabilities-implementation-plan.md) points here for these items; no scheduling task lives in both files. The design authority is the [Time contract](capabilities-contract.md#time-and-userspace-scheduling). Inputs: [`capability-scheduler-design.md`](capability-scheduler-design.md) (design discussion), [`capabilities-scheduling.md`](capabilities-scheduling.md) (survey of existing systems), [`capabilities-time.md`](capabilities-time.md) (Composite comparison). Open questions are only in [`capabilities-decisions.md`](capabilities-decisions.md) (D5, D8). The working rules, validation commands and `just` recipes of the main plan apply unchanged.

Mark an item `- [x]` only after implementation and validation have passed. A finished item leaves a one-line marker in [`capabilities-completed.md`](capabilities-completed.md), and moves to [`scheduling-completed.md`](scheduling-completed.md).

**Model in one paragraph.** Time is a transferable, accounted budget with no priority. The nucleus charges the running Time, enforces limits with a one-shot timer and reports block, expiry and wakeup events to the responsible scheduler. A Thread whose Time is exhausted is ineligible to run until more budget is Transferred in. Every policy (order, priorities, renewal, fairness) is userspace scheduler code.

Phases 1 to 8 are finished, as are the hierarchy items of Phase 9: see [`scheduling-completed.md`](scheduling-completed.md).

## Phase 9 — Hierarchy and domains (what is left)

The decisions and finished items are in [`scheduling-completed.md`](scheduling-completed.md#phase-9--hierarchy-and-domains-done-items).

- [ ] Client ownership APIs that preserve budget on pre-commit errors without relying on destructor side effects (moved here from Phase 8, maintainer decision 2026-10-18, since donation and the strict scheduler tree decide how budget is owned). Candidate shape: an explicit `Loan` handle with `commit` and `refund`, no `Drop`.
- [ ] Temporal service contracts for compositional real-time guarantees, if the project needs them.

**Exit:** storage cannot mint execution authority; delegated time is spent at most once; execution, observation and ownership agree across layers; the nucleus contains no scheduling policy.

## Phase 10 — Retire the kernel queue

After Phase 5. The Threads outside any Scheduler and the Schedulers without a dispatcher are the last users of the first-in, first-out queue.

- [ ] Remove the idle Thread. When nothing is runnable (the dispatcher is parked in `Dispatch(None)`, or the first-in, first-out queue is empty), the kernel idles itself: it executes `wfi` on its own kernel stack with interrupts unmasked, and the next interrupt takes the usual path (handler, activation, switch). Select-next gains a "nothing to run" outcome that waits in the kernel instead of switching to a Thread; `spawn_idle`, `Nucleus::idle_thread` and the idle special cases in `park_and_select` and `preempt_and_select` go, and the test kernels drop their `spawn_idle` calls. The one-shot timer stays disarmed while idle unless a deadline is pending.
- [ ] Meter idle time in the nucleus, since no Time is charged for it: an idle-ticks counter, exact like Time charging, published in the Scheduler record (slot 0) for load measurement.
- [ ] Bring the kernel test fixtures up on a dispatcher, so the test kernels' Threads are dispatched too.
- [ ] Remove the kernel first-in, first-out queue and the pre-dispatcher mechanism.

## Phase 11 — Multicore

**Gated behind future kernel multicore work (maintainer decision, 2026-10-18). Nothing here is addressed now:** the kernel runs on one CPU, secondary-core bring-up is in no phase of this plan, and the questions below stay open in D5 until that work is scheduled.

- [ ] Multicore Times (a Time belongs to one core; Transfer, Merge and dispatch reject another core's Time) and per-CPU schedulers.
- [ ] `Scheduler.replace`: a dispatcher replaces a Thread running on another CPU.
- [ ] Non-blocking dispatch, and the activation invariant of one outstanding request per dispatcher across cores, including a dispatcher already running on another core.
- [ ] Batched remote dispatch.
