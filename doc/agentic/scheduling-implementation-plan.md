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
- [x] Tests: ShareRegion with mapped, unmapped, device, read-only, overlapping and re-shared Frames and a mismatched stride; slot placement; sequence-lock tearing under a concurrent writer; AssignRecord and publication across the Thread lifecycle (start, waiting for budget, funded, retired); budget fields after a refill and against `Time.Query`; a reused slot starting a fresh record; EventCount advancement and the wake of a dispatcher waiting on it; a target test where a checker Thread and an EL0 reader component read a budget-limited spinner's record and the checker reads the EventCount.

## Phase 5 — Dispatch, events and activation (single CPU)

The dispatcher, dispatch and activation rules are in the [Time contract](capabilities-contract.md#time-and-userspace-scheduling). `replace` and activation across cores belong to Phase 9; event notification through the bound EventCount is done in Phase 4.

- [x] `Scheduler.BindDispatcher`: shared ABI, wrapper, object state and dispatch; once-only, the Thread must be configured by the Scheduler and not started, and the bind starts it. While no dispatcher is bound, the Scheduler's Threads run from the first-in, first-out queue, as do the Threads outside any Scheduler (boot, idle, kernel test fixtures); once it is bound they run only when dispatched.
- [x] A root dispatcher component for the production image: a userspace round-robin policy over the record table that Kickstart loads, configures on an infinite Time and binds during boot, with its record Frame and bound EventCount. The nucleus does not charge the root dispatcher to a Time. Done as `userspace/dispatcher` (`root-dispatcher`, a `dispatcher = true` program in the Kickstart manifest): Kickstart shares the record table, hands it the Scheduler and a key per Thread through `DriverInit::dispatch`, and binds it after starting every other Thread. The dispatcher learns of changes through activation, so it binds no EventCount.
- [x] `Scheduler.Dispatch`: blocking, with the registers, activation reason bits and `STALE_GENERATION` status (42, in the shared status table and error decoding) of the contract; the dispatch limit joins budget exhaustion in the one-shot timer's deadline; `x2 = 0` waits while the idle Thread runs.
- [x] Activation: a Thread that blocks, exhausts its budget, yields, faults, exits or reaches its dispatch limit completes the parked dispatcher's call and switches to it; a coalesced pending flag makes a later `Dispatch` return at once.
- [x] `Scheduler.ConfigureActivation`: the mask (`THREAD_RUNNABLE`, `TIME_AVAILABLE`); masked events request activation like the mandatory ones.
- [x] Tests: a round-robin policy on the target with an empty mask; wake preemption with `THREAD_RUNNABLE`; a pending activation returning the next `Dispatch` at once; coalesced bursts; stale, blocked, retired and wrong-Scheduler dispatches; preemption at a `Thread.Return`; with no dispatcher bound, Scheduler-configured Threads run from the first-in, first-out queue (every test kernel but `dispatch-test`).
  - Covered: nucleus tests for bind, dispatch, every activation reason, the mask, a pending activation, refusals and the idle wait, and for a `Thread.Return` followed at once by a tick (the Return is a preemption point like any other kernel exit). On the target, `dispatch-test`: bind (a second bind is refused), round-robin dispatch, limit and exhaustion activation, stale and zero-limit refusals, a refill waking an idle-waiting dispatcher (`TIME_AVAILABLE`), the first-in, first-out boot Thread running while the dispatcher waits, a kicker's wakeup preempting it at once (`THREAD_RUNNABLE`), two wakeups raised while the dispatcher runs coalescing into one pending activation that the next `Dispatch` returns at once, a retired Thread and a Thread no Scheduler configured both refused. The production image boots under the root dispatcher and the shell answers.

## Phase 6 — Timers and timeouts

Needs Phase 3.

- [x] A kernel deadline queue ordered by next event; `CNTP_CVAL` programmed to its head. The queue is the pending-record pool's finite deadlines (`PendingPool::next_deadline`, a scan of at most 32 records) together with the table of dispatcher timers (`objects/timers.rs`); `Nucleus::timer_deadline_ticks` takes the earliest of those and the slice, limit and exhaustion, and `timer::program` writes it. The timer interrupt runs `Nucleus::expire_deadlines`.
- [x] `EventCount.Await` and `Notification.Wait` finite timeouts, completing with `TIMED_OUT` (status 43, `CapError::TimedOut`), then a sleep built on it (`EventCountKey::sleep`).
- [x] `Scheduler.ArmTimer` (op 8): an absolute monotonic deadline in nanoseconds that is an explicit activation request for the dispatcher (`TIMER` reason, not maskable); one timer per dispatcher. Activation timers only; a notify-only timer mode is added only if a real use needs it.
- [x] The virtual counter is sufficient: the nucleus already leaves `CNTKCTL_EL1.EL0VCTEN` set with a zero virtual offset, so EL0 reads the nucleus's clock with `CNTVCT_EL0` (`libuser::clock`). No clock-read operation. Tests: nucleus tests for timeouts, completion-versus-deadline races, the earliest deadline, ArmTimer rules, timer activation with an empty mask and a pending TIMER; host tests for the ABI and `sleep`; `dispatch-test` phase four (the EL0 clock against the kernel's, a timer waking the idle-waiting dispatcher at its deadline, a ten-millisecond Notification timeout, a fifteen-millisecond sleep).

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
- [ ] Child dispatchers: an ordinary Thread on an ordinary Time the parent Transferred in, never replenished by the nucleus. Agent exhaustion makes the agent ineligible and sends the parent a distinct event; issued execution grants stay valid; when a decision is needed and the agent cannot run, control returns to the nearest eligible ancestor.
- [ ] Domain consumption stays bounded because a child holds only budget its parent Transferred and can subdivide it; add parent-enforced effective execution limits only if that proves insufficient.
- [ ] A child's activation requests never override the parent's authority: they notify the parent or use already delegated capacity.
- [ ] Scheduler-crash recovery (suspend the domain, or return control to the parent).
- [ ] Multicore Times, per-CPU schedulers, batched remote dispatch.
- [ ] Temporal service contracts for compositional real-time guarantees, if the project needs them.

**Exit:** storage cannot mint execution authority; delegated time is spent at most once; execution, observation and ownership agree across layers; the nucleus contains no scheduling policy.

## Phase 10 — Retire the kernel queue

After Phase 5. The Threads outside any Scheduler and the Schedulers without a dispatcher are the last users of the first-in, first-out queue.

- [ ] Remove the idle Thread. When nothing is runnable (the dispatcher is parked in `Dispatch(None)`, or the first-in, first-out queue is empty), the kernel idles itself: it executes `wfi` on its own kernel stack with interrupts unmasked, and the next interrupt takes the usual path (handler, activation, switch). Select-next gains a "nothing to run" outcome that waits in the kernel instead of switching to a Thread; `spawn_idle`, `Nucleus::idle_thread` and the idle special cases in `park_and_select` and `preempt_and_select` go, and the test kernels drop their `spawn_idle` calls. The one-shot timer stays disarmed while idle unless a deadline is pending.
- [ ] Meter idle time in the nucleus, since no Time is charged for it: an idle-ticks counter, exact like Time charging, published in the Scheduler record (slot 0) for load measurement.
- [ ] Bring the kernel test fixtures up on a dispatcher, so the test kernels' Threads are dispatched too.
- [ ] Remove the kernel first-in, first-out queue and the pre-dispatcher mechanism.
