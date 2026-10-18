# Scheduling: completed work

The finished items of [`scheduling-implementation-plan.md`](scheduling-implementation-plan.md), moved here as written when they were checked off. The plan keeps what is still open. The one-line markers for each item are in [`capabilities-completed.md`](capabilities-completed.md).

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

The dispatcher, dispatch and activation rules are in the [Time contract](capabilities-contract.md#time-and-userspace-scheduling). `replace` and activation across cores belong to Phase 11; event notification through the bound EventCount is done in Phase 4.

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
- [x] `EventCount.Await` and `Notification.Await` finite timeouts, completing with `TIMED_OUT` (status 43, `CapError::TimedOut`), then a sleep built on it (`EventCountKey::sleep`).
- [x] `Scheduler.ArmTimer` (op 8): an absolute monotonic deadline in nanoseconds that is an explicit activation request for the dispatcher (`TIMER` reason, not maskable); one timer per dispatcher. Activation timers only; a notify-only timer mode is added only if a real use needs it.
- [x] The virtual counter is sufficient: the nucleus already leaves `CNTKCTL_EL1.EL0VCTEN` set with a zero virtual offset, so EL0 reads the nucleus's clock with `CNTVCT_EL0` (`libuser::clock`). No clock-read operation. Tests: nucleus tests for timeouts, completion-versus-deadline races, the earliest deadline, ArmTimer rules, timer activation with an empty mask and a pending TIMER; host tests for the ABI and `sleep`; `dispatch-test` phase four (the EL0 clock against the kernel's, a timer waking the idle-waiting dispatcher at its deadline, a ten-millisecond Notification timeout, a fifteen-millisecond sleep).

## Phase 7 — Receiver Times and wakeups

- [x] Receiver Threads own a Time; binding a Notification or IRQ line requires it, with no fallback to the interrupted Thread's budget. Already so: `IRQHandler.SetNotification` refuses a receiver Thread with no bound Time, and no other operation binds a Notification to a receiver. Interrupt time is never charged to the interrupted Thread (`Nucleus::account_interrupts`).
- [x] A wakeup is reported to the responsible scheduler as an event; the nucleus does not compare Times. A woken dispatcher-managed receiver is published runnable and activates the dispatcher under `THREAD_RUNNABLE` (`Nucleus::make_runnable`), as for any wakeup.
- [x] Per-interrupt accounting (maintainer decision, 2026-10-18): `IRQHandler.Query` (op 4, `READ`) returns a line's interrupt count and nanoseconds; the Scheduler record carries the total `interrupt_ns` (offset 40). The Scheduler record's `timer_expirations` now counts fired dispatcher timers too.
- [x] Tests: an IRQ receiver wakeup reaches the scheduler, which preempts a spinner (by ending its `Dispatch`; `replace` waits for Phase 11); interrupt time is not charged to the interrupted Thread. `dispatch-test` phase five: the virtual timer interrupt wakes a receiver while a spinner runs for up to two seconds, the wakeup alone ends the dispatch within milliseconds, the receiver runs, and `IRQHandler.Query` and the record's `interrupt_ns` show the time. Nucleus tests: `Query` and its rights, the record refresh, and the existing interrupt-accounting tests (the interrupted Thread's interval moves past interrupt time).

## Phase 8 — Thread lifecycle

- [x] Thread lifecycle. A Thread first starts only through `Scheduler.StartThread` (maintainer decision, 2026-10-18: no `Thread.Start`). `Thread.Suspend` (2) and `Thread.Resume` (3), `WRITE`, are a flag on the Thread that keeps its saved state, wait, completion and Time, with explicit legal transitions (contract: Thread control). A suspended Thread is never queued, dispatched or activated for; Resume of an incomplete wait returns it to blocked. `Thread.Retire` on the caller never returns: the next Thread is selected first, the dispatcher is activated with `EXITED`, and the record ends `Dying`. Tests: nucleus tests for the transitions, wakeups while suspended, dispatch refusal, exit with and without something else to run, and the `EXITED` activation; `dispatch-test` phase six on the target.

## Phase 9 — Hierarchy and domains (done items)

Decisions (maintainer, 2026-10-18), recorded in the [Time contract](capabilities-contract.md#time-and-userspace-scheduling):

- **Execution.** The parent's dispatcher dispatches the child's dispatcher with a limit, which is the grant. The child's own `Dispatch` calls are clamped to what is left of it, so child Threads run without a parent activation. When the grant ends, or the child yields or is exhausted, the nucleus stops the running Thread and activates the parent.
- **Adopt.** `Adopt` (on the parent, `WRITE`) sets the parent once and succeeds only on a Scheduler that has no parent, is not the root, and has no children, Threads or dispatcher yet, so the tree stays acyclic without an ancestry walk.
- **Exhaustion.** A child dispatcher is an ordinary Thread of the parent on an ordinary Time; its exhaustion activates the parent with `EXHAUSTED` and shows `AWAITING_BUDGET` in its record. A new `DISPATCHER` flag in the Thread record tells the parent that the Thread runs a domain. No new reason.
- **Crash.** When a child's dispatcher faults or is retired, the nucleus suspends every Thread the child owns (Thread.Suspend's rules), publishes their records as `Suspended`, and activates the parent with a new `SCHED_DOMAIN_FAILED` reason (`1 << 8`). The parent may retire the child, replace its dispatcher or resume the Threads.

- **Donation.** One operation, `MoveThread(thread, destination, slot)`. It is invoked on a Scheduler that is the Thread's owner (donate to a direct child) or the owner's parent (take it back); the destination is the invoked Scheduler's direct child or the invoked Scheduler itself; the caller picks the record slot; it needs `WRITE` on the invoked Scheduler, the Thread and the destination. Legal for any started Thread that is not running, dispatched, the idle Thread or a dispatcher, blocked and suspended ones included: its wait, completion and Time move with it untouched. The old slot is published with a new `ThreadState::Donated` (7).
- **Grant.** The grant is a nucleus field of the child Scheduler (the deadline of the parent's dispatch of its dispatcher), not an entry of a nested dispatch stack; the nucleus finds every live grant by walking the running Thread's Scheduler up its parents. It is shown to the child as `grant_end_ns` in the child's Scheduler record (0 for none), refreshed when the parent dispatches the child dispatcher. When the grant ends while a child Thread runs, that Thread is stopped, the parent is activated with `LIMIT`, the child dispatcher stays parked, and its parked `Dispatch` completes with `LIMIT` when the parent next dispatches it.
- **Depth.** The tree is at most 4 levels deep (the root plus 3).

- **Operations on a non-root Scheduler.** All of them work (`ShareRegion`, `ConfigureThread`, `StartThread`, `AssignRecord`, `BindEventCount`, `BindDispatcher`, `Dispatch`, `ConfigureActivation`, `ArmTimer`) for a holder with `WRITE` on that Scheduler. A fresh child has no execution authority until a parent adopts it and dispatches its dispatcher; its Threads can run only on Times that were Transferred in.
- **Child activation without a live grant.** The domain is live while the parent's dispatch of the child dispatcher is in force. A child activation (after the child's own mask) with a live grant completes the child dispatcher's parked `Dispatch` and runs it at once. Without one, the parked call completes and the child dispatcher becomes an ordinary runnable Thread of the parent: its record is published runnable and the parent's dispatcher is activated only if the parent's mask asks for `THREAD_RUNNABLE`. A child never forces a parent activation.

No design question is open; coding can start.

- [x] Scheduler derivation (`Retype` of an inert Scheduler, `Scheduler.Adopt` (op 9) setting the parent once on a fresh Scheduler, depth at most 3 below the root) and Thread donation to a child with `Scheduler.MoveThread` (op 10), moving the Thread's record to the child's table (the old slot reads `Donated`, 7). Every operation works on a non-root Scheduler; its Threads never use the first-in, first-out queue.
- [x] Child dispatchers: an ordinary Thread of the parent on an ordinary Time the parent Transferred in, never replenished by the nucleus (`BindDispatcher` on a child). The parent's dispatch of it is the grant: the child's `Dispatch` limits are clamped to it, `grant_end_ns` shows in the child's Scheduler record, and when the grant ends the running child Thread is stopped, the parent is activated with `LIMIT` and the child dispatcher's parked call completes with `LIMIT`. A child dispatcher's exhaustion is ordinary exhaustion; its record has the `DISPATCHER` flag.
- [x] Domain consumption stays bounded because a child holds only budget its parent Transferred and can subdivide it, and its Threads' limits are clamped to the grant. Parent-enforced effective execution limits are not needed so far.
- [x] A child's activation requests never override the parent's authority: with a grant in force the child dispatcher runs at once inside it; without one the child dispatcher becomes an ordinary runnable Thread of the parent, announced only as the parent's mask allows.
- [x] Scheduler-crash recovery: when a child's dispatcher faults or is retired the nucleus suspends the child's Threads, ends its grant and timer, unbinds the dispatcher (a new one can be bound) and activates the parent with `SCHED_DOMAIN_FAILED` (`1 << 8`, mandatory).

## Phase 10 — Retire the kernel queue

Decisions (maintainer, 2026-10-18):

- **Fixtures.** The kernel test fixtures run on the real dispatch path. A fixture dispatcher in `libkicktest` (an `EL1t` Thread using `Scheduler.Dispatch`; its policy needs no records: it tries each registered fixture Thread in turn and skips the ones `Dispatch` refuses, and waits with nothing dispatched when none ran) serves the custom test kernels; fixture Threads, the boot Thread included, become Threads of the root Scheduler through a test helper. The test kernels that load drivers through Kickstart bundle the production `root-dispatcher` instead, and Kickstart registers the boot Thread with it.
- **Idle.** The kernel idles itself: inside the trap handler it unmasks interrupts and executes `wfi`, with a restricted interrupt path for the current exception level (claim, deliver, account, expire deadlines; no preemption) that returns to the waiting loop, which then selects the next Thread and rewrites the original trap frame.
- **Order.** Stage 1: fixtures on a dispatcher, the queue still present. Stage 2: remove the queue and the pre-dispatcher mechanism, with an activation slot in its place. Stage 3: remove the idle Thread and meter idle time. Each stage ends with the whole suite green.

After Phase 5. The Threads outside any Scheduler and the Schedulers without a dispatcher are the last users of the first-in, first-out queue.

- [x] Remove the idle Thread. When nothing is runnable the kernel idles itself: the scheduling transaction (`park_and_select`, `preempt_and_select`, `exit_current_thread`, `park_faulted_and_select`) returns `Switch::Idle` after committing the stop, with no Thread current; the trap handler (`idle_until_runnable` in `kernel/nucleus/src/main.rs`) loops on `Nucleus::select_after_idle`, waiting with `wfi` on the kernel stack and unmasking interrupts only to take the pending one. An interrupt taken there (`current_elx_irq`, only while the idle flag is set) claims, delivers, accounts and expires deadlines, with no preemption. `spawn_idle`, `Nucleus::idle_thread`, `idle_is_current` and the idle special cases are gone, and the test kernels no longer create an idle Thread. The one-shot timer stays disarmed while idle unless a wait timeout or dispatcher timer is pending. An exhausted Thread stops even when nothing else can run, and the last Thread may retire itself.
- [x] Meter idle time in the nucleus, since no Time is charged for it: `Nucleus::idle_ticks`, exact like Time charging (the interval since the stop, minus the interrupt time accounted during it), published as `idle_ns` (offset 56) in the Scheduler record when the wait ends. Exercised by the nucleus tests (`idle_time_is_metered_apart_from_interrupt_time`, `parking_with_nothing_runnable_idles_and_the_next_runnable_thread_ends_it`) and by `dispatch-test`, which asserts `idle_ns > 0`.
- [x] Bring the kernel test fixtures up on a dispatcher, so the test kernels' Threads are dispatched too.
- [x] Remove the kernel first-in, first-out queue and the pre-dispatcher mechanism.
