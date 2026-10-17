# Scheduling implementation plan

The checklist for Time and userspace scheduling. [`capabilities-implementation-plan.md`](capabilities-implementation-plan.md) points here for these items; no scheduling task lives in both files. The design authority is the [Time contract](capabilities-contract.md#time-and-userspace-scheduling). Inputs: [`capability-scheduler-design.md`](capability-scheduler-design.md) (design discussion), [`capabilities-scheduling.md`](capabilities-scheduling.md) (survey of existing systems), [`capabilities-time.md`](capabilities-time.md) (Composite comparison). Open questions are only in [`capabilities-decisions.md`](capabilities-decisions.md) (D5, D8). The working rules, validation commands and `just` recipes of the main plan apply unchanged.

Mark an item `- [x]` only after implementation and validation have passed. A finished item leaves a one-line marker in [`capabilities-completed.md`](capabilities-completed.md).

**Model in one paragraph.** Time is a transferable, accounted budget with no priority. The nucleus charges the running Time, enforces limits with a one-shot timer and reports block, expiry and wakeup events to the responsible scheduler. A Thread whose Time is exhausted is ineligible to run until more budget is Transferred in. Every policy (order, priorities, renewal, fairness) is userspace scheduler code.

## Phase 1 — Execution accounting

Kernel-private; no ABI change.

- [x] Per-Thread run time (`Thread::run_time_ns`) charged at every switch-out in `switch_away`, from `Nucleus::current_since_ns`.
- [ ] Test a Thread charged across several switches.

## Phase 2 — Remove kernel priorities from Time

The contract no longer has a priority chain. Remove it everywhere in one slice; there is no compatibility with the old schema.

- [x] Shared ABI (`libs/object/src/time.rs`): drop the chain, `DEFAULT_PRIORITY`/`LOWEST_PRIORITY`, Transfer's `x4` and Query's `x2` priority; drop the `PRIORITY_CHAIN_FULL` status and renumber the later statuses (`libs/object/src/syscall_status.rs`) with the literal ABI tests.
- [x] Wrappers and kernel: `kernel/nucleus/src/objects/time.rs`, `kernel/nucleus/src/api/time.rs`, the boot Time's self-chain entry in Kickstart.
- [x] Remove the manifest `priority` fields and their plumbing (`kernel/kickstart/image.toml`, `libs/image-build`, `kernel/kickstart/src/drivers.rs`); drivers keep their `budget_ns`.
- [x] Update `doc/object_types/core/time.md` and the catalogue in `doc/object_types/README.md` from the code; fix the Time entries in `capabilities-completed.md`.
- [x] Tests: Transfer and Merge conservation without priorities, per-core rejection, infinite budgets, reserved-word rejection, literal status values.

**Exit (met):** a Time is a budget and nothing else, in the contract, the code and the object-type documentation. A program in an image manifest is marked `program = true`; drivers carry only `budget_ns`, `memory`, `video_memory` and `serial`.

## Phase 3 — Charge the bound Time and enforce limits

Needs the contract's accounting rules (no per-charge rounding; kernel time charged; interrupt time excluded).

- [ ] Charge elapsed counter ticks to the running Thread's bound Time (`Thread::time`, set by `Scheduler.ConfigureThread`), keeping the fractional part; an infinite Time never decreases.
- [ ] Convert and round the accumulated value only at limit checks and observations.
- [ ] Exclude interrupt handling time from the running Time; account it separately (open in D8).
- [ ] Ineligibility: a Thread whose Time is exhausted leaves the run queue until budget is Transferred in, and re-enters it then. It is not a Thread state.
- [ ] One-shot timer in place of the free-running tick (`timer::arm_at`): the earlier of the slice end and budget exhaustion; no timer while only the idle Thread is runnable.
- [ ] Tests: conservation, expiry, an infinite Time, a Time drained by Transfer while its Thread runs, a Thread made eligible again by a Transfer, boot Time unaffected, no charge for interrupt time. Target scheduling test with two Threads on finite Times.

## Phase 4 — Scheduler-shared records (D5)

Needs the D5 questions in `capabilities-decisions.md`.

- [ ] Implement `Scheduler.ShareRegion` across shared ABI, wrapper, dispatch and object state: one already scheduler-mapped Frame per call, reject conflicting re-share, check Scheduler and Frame authority.
- [ ] Implement `Scheduler.BindEventCount`: bind an EventCount to a Scheduler, with the designated agent as its only waiter; reject a second binding.
- [ ] Implement the scheduler-declared fixed-stride record table: capacity, Thread identity/incarnation lookup, publication/snapshot protocol and record reuse. Kernel-private execution state stays private. `ThreadKey::state`, `time_used_ns` and `pending_notifications` (`libs/object/src/thread.rs`) read these records instead of panicking through `unimplemented!()`.
- [ ] Implement hierarchical Scheduler derivation, root-only Thread creation and Thread donation with strict tree structure; Kickstart establishes the root scheduler before scheduling starts.
- [ ] Move notification and block-reason accounting into the shared records, with event-summary indexing and Thread-retirement cleanup.
- [ ] Align consumed, remaining and activation observations with the records.
- [ ] Test scheduler/Frame authority, mapped and unmapped ShareRegion inputs, re-share rejection, stride placement, identity reuse, tree constraints, root creation/donation, publication/reuse and Thread teardown; run target checks for record access and kernel-private state isolation.

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

- [ ] Child scheduler agents: an ordinary Thread on an ordinary Time the parent Transferred in, never replenished by the nucleus. Agent exhaustion makes the agent ineligible and sends the parent a distinct event; issued execution grants stay valid; when a decision is needed and the agent cannot run, control returns to the nearest eligible ancestor.
- [ ] Domain consumption stays bounded because a child holds only budget its parent Transferred and can subdivide it; add parent-enforced effective execution limits only if that proves insufficient.
- [ ] A child's activation requests never override the parent's authority: they notify the parent or use already delegated capacity.
- [ ] Scheduler-crash recovery (suspend the domain, or return control to the parent).
- [ ] Multicore Times, per-CPU schedulers, batched remote dispatch.
- [ ] Temporal service contracts for compositional real-time guarantees, if the project needs them.

**Exit:** storage cannot mint execution authority; delegated time is spent at most once; execution, observation and ownership agree across layers; the nucleus contains no scheduling policy.
