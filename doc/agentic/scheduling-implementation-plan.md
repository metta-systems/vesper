# Scheduling implementation plan

The checklist for Time and userspace scheduling. [`capabilities-implementation-plan.md`](capabilities-implementation-plan.md) points here for these items; no scheduling task lives in both files. The design authority is the [Time contract](capabilities-contract.md#time-and-userspace-scheduling). Inputs: [`capability-scheduler-design.md`](capability-scheduler-design.md) (design discussion), [`capabilities-scheduling.md`](capabilities-scheduling.md) (survey of existing systems), [`capabilities-time.md`](capabilities-time.md) (Composite comparison). Open questions are only in [`capabilities-decisions.md`](capabilities-decisions.md) (D5, D8). The working rules, validation commands and `just` recipes of the main plan apply unchanged.

Mark an item `- [x]` only after implementation and validation have passed. A finished item leaves a one-line marker in [`capabilities-completed.md`](capabilities-completed.md).

**Model in one paragraph.** Time is a transferable, accounted budget with no priority. The nucleus charges the running Time, enforces limits with a one-shot timer and reports block, expiry and wakeup events to the responsible scheduler. A Thread whose Time is exhausted is ineligible to run until more budget is Transferred in. Every policy (order, priorities, renewal, fairness) is userspace scheduler code.

## Phase 1 — Execution accounting

Kernel-private; no ABI change.

- [x] Per-Thread run time (`Thread::run_time_ns`) charged at every switch-out in `switch_away`, from `Nucleus::current_since_ns`.
- [x] Test a Thread charged across several switches (`preempt-test`: the limited EL0 spinner accumulates its run time over several slices).

## Phase 2 — Remove kernel priorities from Time

The contract no longer has a priority chain. Remove it everywhere in one slice; there is no compatibility with the old schema.

- [x] Shared ABI (`libs/object/src/time.rs`): drop the chain, `DEFAULT_PRIORITY`/`LOWEST_PRIORITY`, Transfer's `x4` and Query's `x2` priority; drop the `PRIORITY_CHAIN_FULL` status and renumber the later statuses (`libs/object/src/syscall_status.rs`) with the literal ABI tests.
- [x] Wrappers and kernel: `kernel/nucleus/src/objects/time.rs`, `kernel/nucleus/src/api/time.rs`, the boot Time's self-chain entry in Kickstart.
- [x] Remove the manifest `priority` fields and their plumbing (`kernel/kickstart/image.toml`, `libs/image-build`, `kernel/kickstart/src/drivers.rs`); drivers keep their `budget_ns`.
- [x] Update `doc/object_types/core/time.md` and the catalogue in `doc/object_types/README.md` from the code; fix the Time entries in `capabilities-completed.md`.
- [x] Tests: Transfer and Merge conservation without priorities, per-core rejection, infinite budgets, reserved-word rejection, literal status values.

**Exit (met):** a Time is a budget and nothing else, in the contract, the code and the object-type documentation. A program in an image manifest is marked `program = true`; drivers carry only `budget_ns`, `memory`, `video_memory` and `serial`.

## Phase 3 — Charge the bound Time and enforce limits

The contract's accounting rules apply: ticks kept exact, rounded up only at limit checks; kernel time charged; interrupt time excluded.

- [x] Charge elapsed counter ticks to the running Thread's bound Time (`Thread::time`, set by `Scheduler.ConfigureThread`), keeping the fractional part; an infinite Time never decreases.
- [x] Convert and round the accumulated value only at limit checks and observations.
- [x] Exclude interrupt handling time from the running Time; record it per interrupt line (a count and the ticks spent) in the line table.
- [x] Ineligibility: a Thread whose Time is exhausted leaves the run queue until budget is Transferred in, and re-enters it then. It is not a Thread state.
- [x] One-shot timer in place of the free-running tick (`timer::program`, reprogrammed at every kernel exit): the earlier of the slice end and budget exhaustion; no timer while only the idle Thread is runnable.
- [x] Tests: conservation, expiry, an infinite Time, a Time drained by Transfer while its Thread runs, a Thread made eligible again by a Transfer, boot Time unaffected, no charge for interrupt time. Target scheduling test with two Threads on finite Times.

## Phase 4 — Scheduler-shared records (D5)

The record ABI is in the [contract](capabilities-contract.md#thread-and-addressspace-contracts). Hierarchical derivation and donation are in Phase 9.

- [x] Shared ABI (`libs/object`): `SchedulerOp` gains `AssignRecord` and `BindEventCount`; the `SchedulerRecord` and `ThreadRecord` layouts with size and offset assertions, the `ThreadState` mapping, flag, block-reason and stop-reason codes, and the seqlock snapshot helpers (`ThreadRecordView`, `SchedulerRecordView`); the panicking `ThreadKey::state`, `time_used_ns` and `pending_notifications` stubs are removed.
- [x] `Scheduler.ShareRegion`: wrapper, dispatch and object state (the record AddressSpace, the stride and up to 8 regions in the carved Scheduler); one mapped RAM Frame per call, re-share and overlap rejected, authority checked.
- [x] Record publication: the nucleus is the single writer of both record kinds through the kernel mapping, with the seqlock protocol; Frame identity is validated before each write.
- [x] `Scheduler.AssignRecord`: link a Thread to a slot and publish its record at once; publish on every Thread state change (start, queued, awaiting budget, running, blocked, stopped, faulted, retired) with block-reason accounting and `last_stop`/`last_run_ns`; `Dying` on Thread retirement, and the slot association dropped.
- [x] `Scheduler.BindEventCount`: once-only; the nucleus advances the EventCount after publishing a state change, not for counter-only refreshes; a retired EventCount ends the binding.
- [x] Tests: ShareRegion with mapped, unmapped, device, read-only, overlapping and re-shared Frames and a mismatched stride; slot placement; sequence-lock tearing under a concurrent writer; AssignRecord and publication across the Thread lifecycle (start, waiting for budget, funded, retired); budget fields after a refill and against `Time.Query`; a reused slot starting a fresh record; EventCount advancement and the wake of an agent waiting on it; a target test where a checker Thread and an EL0 reader component read a budget-limited spinner's record and the checker reads the EventCount.

## Phase 5 — Dispatch, replace, events and activation

Needs the D5/D8 schema questions (`dispatch`, `replace`, `ConfigureActivation`, event records). The activation contract is in [the Time contract](capabilities-contract.md#time-and-userspace-scheduling).

- [ ] `Scheduler.dispatch(thread, time, limit)` and `Scheduler.replace(cpu, thread, time, limit)` with the expected-generation check, caller scheduling authority, spendable-Time and CPU authorization checks, and a rejection that returns the current state.
- [ ] Event notification: for a scheduling-relevant transition the nucleus publishes the records, then advances the bound EventCount with release ordering. Advance only for scheduling-relevant transitions, never for plain accounting updates. The agent reads the counter before the records and waits with `EventCount.Await(observed + 1)`. Advancement carries no scheduling privilege.
- [ ] Event records for information that cannot be rebuilt from state (timer expiry, why an execution stopped and the amount charged), kept apart from coalescible state-change notifications, with an overflow rule.
- [ ] Mandatory activation requests for a running Thread that blocks, exhausts its Time, yields or terminates when a new dispatch decision is needed.
- [ ] `Scheduler.ConfigureActivation(mask)` with `THREAD_RUNNABLE` and `TIME_AVAILABLE` (a Time going from exhausted to spendable); an empty mask means notify only.
- [ ] Activation requests coalesce to at most one outstanding per scheduler agent (waiting, activation pending, running), through one pending-state protocol shared with the EventCount wait so a notification arriving while the agent prepares to sleep is not lost. A request is not a mandatory context switch; the scheduler then issues `dispatch` or `replace`.
- [ ] Root scheduler: privileged activation, no Time, execution accounted separately, no fallback scheduler in the nucleus. The optional diagnostic activation quantum is not part of this item.
- [ ] The root scheduler dispatches Threads; the kernel FIFO runs only until the root scheduler is established.
- [ ] Charge PPC per-call time on Return once `current_time_ns()` reads a real clock (the client's Time pays).
- [ ] Tests: a userspace round-robin policy example on the target (empty mask); a fixed-priority example with `THREAD_RUNNABLE` that preempts on wake; no lost wakeup when an event lands between the counter read and `Await`; coalesced activations under a wakeup burst; adversarial dispatch and replace of a stale, blocked, retired or foreign-core Thread; cancellation; nested delegation and simultaneous spending.

## Phase 6 — Timers and timeouts

Needs Phase 3.

- [ ] A kernel deadline queue ordered by next event; `CNTP_CVAL` programmed to its head.
- [ ] `EventCount.Await` finite timeout (currently `InvalidOperation`), then a sleep built on it.
- [ ] `Scheduler.arm_timer(deadline)`: an absolute monotonic deadline that is an explicit activation request for the scheduler. Activation timers only; a notify-only timer mode is added only if a real use needs it.
- [ ] Expose a clock read to EL0 only if the virtual counter is not sufficient.

## Phase 7 — Receiver Times and wakeups

- [ ] Receiver Threads own a Time; binding a Notification or IRQ line requires it, with no fallback to the interrupted Thread's budget.
- [ ] A wakeup is reported to the responsible scheduler as an event; the nucleus does not compare Times.
- [ ] Per-interrupt accounting once its design is decided.
- [ ] Tests: an IRQ receiver wakeup reaches the scheduler, which preempts a spinner by `replace`; interrupt time is not charged to the interrupted Thread.

## Phase 8 — Thread lifecycle

- [ ] Thread `Start`/`Suspend`/`Resume` with initialized contexts, explicit authority, legal transitions and preserved blocked completions and budget; never-returning self-retirement.
- [ ] Client ownership APIs that preserve budget on pre-commit errors without relying on destructor side effects.

## Phase 9 — Hierarchy, domains and multicore

- [ ] Scheduler derivation (`Retype` of an inert Scheduler, an `Adopt` operation that sets its parent once and keeps the tree strict) and Thread donation to a child, moving the Thread's record to the child's table.
- [ ] Child scheduler agents: an ordinary Thread on an ordinary Time the parent Transferred in, never replenished by the nucleus. Agent exhaustion makes the agent ineligible and sends the parent a distinct event; issued execution grants stay valid; when a decision is needed and the agent cannot run, control returns to the nearest eligible ancestor.
- [ ] Domain consumption stays bounded because a child holds only budget its parent Transferred and can subdivide it; add parent-enforced effective execution limits only if that proves insufficient.
- [ ] A child's activation requests never override the parent's authority: they notify the parent or use already delegated capacity.
- [ ] Scheduler-crash recovery (suspend the domain, or return control to the parent).
- [ ] Multicore Times, per-CPU schedulers, batched remote dispatch.
- [ ] Temporal service contracts for compositional real-time guarantees, if the project needs them.

**Exit:** storage cannot mint execution authority; delegated time is spent at most once; execution, observation and ownership agree across layers; the nucleus contains no scheduling policy.
