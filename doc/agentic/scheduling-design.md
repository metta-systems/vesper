# Scheduling design

The rules for Time and userspace scheduling. Open questions are in
[scheduling-decisions.md](scheduling-decisions.md), open work in
[scheduling-implementation-plan.md](scheduling-implementation-plan.md), finished work in
[scheduling-completed.md](scheduling-completed.md). Background, not specification: the
[design discussion](#background-design-discussion-not-specification) at the end of this
document (proposals, not rules: only the Decisions section is approved), and [scheduling-research.md](scheduling-research.md) (survey of existing
systems and the Composite tcap comparison).

Capability semantics (keys, rights, Retype, the object operation baseline) are in the
[capabilities contract](capabilities-contract.md); multicore rules (per-core Schedulers, dispatchers,
boot Times and deadline queues, the cross-core Thread move) are in the [SMP
design](smp-design.md#decisions).

## Decisions

Time is a first-class capability to a bounded CPU budget, not just a timer object. The nucleus enforces budget consumption, execution limits, preemption on a scheduler's request and authorized transitions. Userspace schedulers hold all policy: which Thread runs next, when budget is renewed, priorities, deadlines and fairness. The nucleus has no priorities, periods, deadlines or scheduling classes.

The nucleus owns the tick: the EL1 non-secure physical timer (`CNTP`), whose interrupt it takes through the privileged interrupt-controller component (see [`irq-controller-placement.md`](irq-controller-placement.md) and [IRQ delivery](capabilities-contract.md#irq-delivery)). Implementation status: the nucleus charges the running Thread's bound Time in exact counter ticks, stops a Thread whose Time is exhausted and announces it to its dispatcher when budget arrives, counts interrupt time per line without charging it to the interrupted Thread, and programs the one-shot physical timer for the earliest of the dispatch limit, budget exhaustion and the next deadline; the nucleus has no run queue, only dispatchers run Threads. `replace` waits for multicore.

### Selected model (maintainer decision, 2026-10-17)

Time is a transferable, accounted CPU budget; scheduling policy lives in userspace schedulers; the nucleus keeps no queue of runnable Threads.

- **Budget only.** A Time holds a remaining budget. It carries no priority, chain, period or deadline.
- **Transfer only.** Moving budget into another Time is permanent. There is no loan, no parent link and no automatic return: unused budget goes back only by an explicit Transfer or Merge.
- **Replenishment is userspace policy.** The nucleus never renews a Time. A scheduler renews budget by Transferring it in, so round-robin quanta, sporadic servers, EDF reservations and fair shares are all scheduler code.
- **Dispatch and replace.** `Scheduler.dispatch(thread, time, limit)` runs a Thread on a Time for at most `limit`. `Scheduler.replace(cpu, thread, time, limit)` atomically stops the Thread running on the CPU, records its remaining allowance and runs another, rejecting the request with the current state when the expected generation no longer matches (stale decisions). The nucleus checks that the caller has scheduling authority over the Thread, that the Time is spendable and allows the limit, that the Thread is runnable and not executing elsewhere, and that the CPU is authorized. A protected procedure call keeps the current Time, so the client pays for the server's work. The execution interval ends on budget exhaustion, reaching the limit, blocking, yielding, a scheduler-requested replacement or Thread retirement.
- **Tickless enforcement.** The nucleus charges the elapsed time to the running Time and programs a one-shot timer for the earlier of budget exhaustion and the limit.
- **Exhaustion.** A Thread whose Time is exhausted is ineligible to run until more budget is Transferred into the Time. This is not a Thread state: the nucleus simply does not run it. Whether to fund it again is the scheduler's decision. A Transfer or Merge that refills an ineligible Thread's Time announces the Thread to its dispatcher. A Thread with no bound Time (the boot Thread) is never exhausted.
- **Accounting (maintainer decision, 2026-10-17).** Elapsed counter ticks are attributed to the running Time without rounding each charge: the Time keeps the exact tick count. The accumulated charge is converted to nanoseconds, rounded up, only when it is compared with a limit or reported, so a Thread never runs past its budget and Query reports the remaining budget rounded down. Kernel time spent on the running Thread's behalf, such as system calls, is charged to the running Time. Time spent handling interrupts is not charged to it; the nucleus records it per interrupt line (a count and the nanoseconds spent) in the line table, which `IRQHandler.Query` reports, and in total in the Scheduler record (`interrupt_ns`) (maintainer decision, 2026-10-18). The line it keeps for its own timer and lines with no handler are recorded the same way, and the receiver Thread's own work is charged to its own Time. Every Thread's run time is metered kernel-privately, and the time the nucleus spends idle (nothing runnable, waiting for an interrupt, not counting the interrupts handled meanwhile) is metered exactly like Time charging and published in the Scheduler record (`idle_ns`), so idle time measures load.
- **Receiver Threads.** A Thread woken by the kernel (Notification or IRQ delivery) owns a Time by construction: binding a Notification to a line or a receiver role requires the receiver's Time, so there is no fallback to the interrupted Thread's budget. The nucleus does not decide whether a wakeup preempts the running Thread: it reports the wakeup as a scheduling event to the responsible scheduler, which decides.
- **State, notification and activation (maintainer decision, 2026-10-17).** Three mechanisms are kept apart, and the nucleus guarantees only that a scheduler can regain control; it does not switch to the scheduler on every event.
  - *State.* The authoritative per-Thread and per-Time state (run state, consumed and remaining budget, a generation) lives in the scheduler-shared records, which the nucleus publishes with sequence numbers so a scheduler can detect a stale observation. Every transition updates the records whether or not it notifies or activates. A multi-field record needs an explicit publication protocol (sequence-lock style); a sequence counter alone is not an atomic snapshot.
  - *Notification.* A Scheduler is bound to an EventCount (`Scheduler.BindEventCount`). For a scheduling-relevant transition the nucleus first publishes the records, then advances the EventCount with release ordering. The dispatcher reads the counter before it reads the records, processes them, then calls `EventCount.Await(observed + 1)`; an event that arrived in between has already advanced the counter, so no wakeup is lost. Only scheduling-relevant transitions advance the counter, not every accounting update. State-change notifications may coalesce. Events whose information cannot be rebuilt from current state, such as a timer expiry or why an execution stopped, need records of their own. The scheduler's dispatcher is the only waiter on its EventCount. Advancing the EventCount is an ordinary waiter wakeup and carries no scheduling privilege.
  - *Activation.* Activation is a property of the Scheduler domain, not an ordinary wakeup of its dispatcher Thread. A running Thread that blocks, exhausts its Time, yields or terminates always requests activation of its responsible scheduler when a new dispatch decision is needed. A scheduler-level activation mask (`Scheduler.ConfigureActivation`) selects which other events request activation: `THREAD_RUNNABLE` (a Thread became runnable) and `TIME_AVAILABLE` (a Time went from exhausted to spendable). An empty mask means notify only, so a scheduler that wants no wakeup preemption bounds its wake latency by the `limit` it dispatches with. `Scheduler.ArmTimer` takes an absolute monotonic deadline and is an explicit request to activate the scheduler then (see below); the `TIMER` reason is mandatory, whatever the mask. An activation is a request, not a mandatory context switch: requests coalesce to at most one outstanding per dispatcher (dispatcher states waiting, activation pending, running), and the scheduler then issues `dispatch` or `replace` as it chooses. A request asks for the earliest activation the scheduler's domain authority permits; urgency is not a right to CPU time.
  - *Dispatcher (maintainer decision, 2026-10-17).* A Scheduler's dispatcher is the Thread that makes its scheduling decisions, and it is bound explicitly. `Scheduler.BindDispatcher` (`5`): `x2` Thread key (`WRITE`), `x3..x7` zero; `WRITE` on the Scheduler. The Thread must be configured by this Scheduler and not started; the bind is once-only, and the bind starts the dispatcher. Kickstart binds the root dispatcher during boot. A Scheduler with no dispatcher bound runs nothing: its Threads wait (maintainer decision, 2026-10-18; there is no kernel run queue). Kickstart registers the boot Thread with the root dispatcher, and the kernel test fixtures run on a dispatcher too (the production one, or a fixture dispatcher in `libkicktest`). The kernel keeps only the list of dispatchers an activation has made ready to run next. A dispatcher that dispatched nothing (`x2` 0) is activated by any event that makes a Thread runnable, whatever the mask, since the CPU would otherwise idle with work pending.
  - *Dispatch.* `Scheduler.Dispatch` (`6`) is invoked by the dispatcher only, and it blocks: on one CPU the dispatcher cannot run while it dispatches. `x2` Thread key, or 0 to run nothing and just wait (the kernel idles meanwhile if nothing else runs); `x3` the Thread's own bound Time key, or 0 for the same; `x4` the limit in nanoseconds; `x5` the expected generation from the Thread's record; `x6` the CPU, which must be 0; `x7` zero; `WRITE` on the Scheduler and the Thread. The Thread runs on its bound Time for at most the limit. The call returns when the nucleus activates the dispatcher, or at once if an activation is already pending: `x1` holds the activation reason bits (event pending, limit reached, budget exhausted, blocked, yielded, faulted, exited, timer, domain failed) and `x2` the scheduler's event count. A Thread whose generation differs from `x5` is not run and the call fails with `STALE_GENERATION` (status 42), `x1` the Thread's current generation. `Scheduler.replace` waits for multicore, where a dispatcher can replace a Thread running on another CPU. Dispatching a Time other than the Thread's bound one is later work.
  - *ConfigureActivation.* `Scheduler.ConfigureActivation` (`7`): `x2` the mask (bit 0 `THREAD_RUNNABLE`, bit 1 `TIME_AVAILABLE`), `x3..x7` zero; `WRITE` on the Scheduler.
  - *ArmTimer (maintainer decision, 2026-10-17).* `Scheduler.ArmTimer` (`8`) is invoked by the bound dispatcher only: `x2` the deadline in nanoseconds on the monotonic clock below (0 disarms), `x3..x7` zero; `WRITE` on the Scheduler. A dispatcher has one timer: arming replaces the previous deadline. It returns `x1` the current time in nanoseconds and `x2` the previous deadline (0 if none). When the clock reaches the deadline the timer disarms itself and requests activation with the `TIMER` reason (`1 << 7`), which the mask cannot turn off; a deadline already in the past fires at the next kernel exit. The timer belongs to the dispatcher and ends with it. Activation timers only: a notify-only timer is added if a real use needs it. Timer deadlines and wait timeouts share the kernel's deadline queue, whose head the one-shot hardware timer is programmed to.
  - *Clock (maintainer decision, 2026-10-17).* The monotonic clock is the physical counter in nanoseconds since the counter started; the kernel converts with 128-bit arithmetic and rounds a deadline up. EL0 reads the same clock without a system call: `CNTVCT_EL0` (virtual offset 0, so equal to the physical counter) and `CNTFRQ_EL0`; the nucleus leaves `CNTKCTL_EL1.EL0VCTEN` set and never changes the virtual offset.
  - *Return is not special.* A protected call keeps the Thread's Time, so the client's Time already pays for the server's work; `Thread.Return` is one more point at which a Thread may be preempted, with no charge of its own.
  - *Root scheduler.* Its dispatcher runs under privileged scheduling authority, needs no Time and is not subject to budget exhaustion; its execution is accounted separately. The nucleus has no fallback scheduler. A bounded activation quantum is an optional diagnostic and fault-containment measure, not part of scheduling semantics.
  - *Root dispatcher policy: Thread priority (maintainer decisions, 2026-10-29).* The maintainer's words: "declare priority with the thread, two levels should be enough for now, keep room for extending in the future, no starvation guard for this round"; and, after the board showed sluggish and lost keys with audio playing: "the usb is probably in need of priority between the main app and the audio?"; and, after a raised USB Thread that spun froze the board (2026-10-29), reversing the first decision's last clause: "delay_ms(1) should just park the thread until 1ms passes, not busy wait / add starvation guard too". This is userspace and Kickstart policy only; the nucleus stays priority-free (no kind, op or record field is added). A Thread's priority is declared where the Thread is declared (`libdevice_driver::Priority`, a one-byte level: 0 normal, 1 input, 2 high, the other values free for more levels): a further Thread's in `driver_threads!`, carried in the 16-byte entries of the `.driver_threads` section, and a component's first Thread's in `entry_priority!`, carried in the one-byte `.entry_priority` section (normal without it). Kickstart hands each managed Thread's level to the root dispatcher in `DispatchInit::priorities`, indexed by record slot. The root dispatcher always dispatches a runnable Thread of the highest level and round-robins within a level, each level keeping its own turn. High is for Threads with a deadline (Wood's audio playback Thread); input is for the cheap Threads on a key's path from the USB controller to the focused window (the DWC2 component's interrupt and poller Threads, the HID component's Thread, the compositor's input Thread), so audio stays ahead of input and input ahead of the CPU-bound programs. Starvation guard: a Thread above normal that has executed for a whole slice (10 ms) since it last blocked is dispatched as a normal Thread, round-robin with the normal ones, until it blocks again. Execution adds up across preemptions and yields; only a stop by blocking, exiting or faulting starts the count over. The dispatcher keeps this account from existing information only: after each `Dispatch` it reads the Thread's record, `run_time_ns` (the increase since the last read is the run) and `last_stop` (`Blocked`, `Exited` or `Faulted` reset the count). A raised Thread that blocks before it has used a slice keeps its level, so a woken high Thread still runs at once. The guard is a safety net, not a licence: every Thread above normal must still block most of the time and do little work each time it wakes, and a driver pauses by parking (a timed wait) rather than spinning.
  - *Child schedulers (maintainer decisions, 2026-10-18).* The parent's dispatcher dispatches a child's dispatcher with a limit, which is the child's execution grant; the child's own `Dispatch` limits are clamped to the grant that is left, and the nucleus stops the running Thread and activates the parent when the grant ends or the child yields or is exhausted. `Adopt` (on the parent, `WRITE`) sets a Scheduler's parent once and only for a fresh Scheduler: no parent, not the root, no children, Threads or dispatcher. A child dispatcher's exhaustion is ordinary exhaustion, with a `DISPATCHER` flag in the Thread record. When a child's dispatcher faults or is retired the nucleus suspends the child's Threads and activates the parent with `SCHED_DOMAIN_FAILED` (`1 << 8`). A child's activation with a live grant (its dispatcher is the Thread the parent dispatched) completes the dispatcher's parked `Dispatch` and runs it at once; without one it makes the child dispatcher an ordinary runnable Thread of the parent, activating the parent only as the parent's mask allows. A Thread moves between a Scheduler and its direct child with `MoveThread` (the old record slot reads `Donated`); the tree is at most 4 levels deep; every Scheduler operation works on a non-root Scheduler for a holder with `WRITE`. The details are in the [scheduling plan](scheduling-implementation-plan.md), Phase 9. A child's dispatcher is an ordinary Thread running on an ordinary Time that its parent Transferred in; the nucleus never replenishes it. When it is exhausted the dispatcher is ineligible and the parent receives an event, distinct from an ordinary Thread's exhaustion, and may Transfer more, leave the child suspended, replace the dispatcher or revoke the child. An already issued execution grant is unaffected; when a new decision is needed and the dispatcher cannot run, control returns to the nearest eligible ancestor. Funding the dispatcher and the workload from separate Times is a userspace convention, not a kernel distinction. A domain's consumption is bounded because a child holds only budget its parent Transferred, which it can subdivide further.
- **Per core.** A Time belongs to one core; Transfer, Merge and dispatch reject a Time of another core.
- **No minting.** Retype creates an empty Time. Positive budget comes only from Transfer out of a Time that holds it; the root budget is the boot Time described below.

Operation vocabulary: **Transfer** moves an amount of budget into a destination Time; **Merge** moves all remaining budget out of a source Time and leaves it empty; **Query** returns the remaining budget. A child budget is an empty Time plus a Transfer. The `replace` register schema, rights and generation encoding are open ([scheduling-decisions.md](scheduling-decisions.md)).

### Time representation and schemas (maintainer decision, 2026-10-17)

- **Units.** Budgets are `u64` nanoseconds on the wire and inside the nucleus; `u64::MAX` is an infinite budget, which never decreases.
- **Creation.** `Untyped.Retype` creates a Time with `size_bits` reserved zero, carved from the invoking Untyped like every Retype-created object (see [allocation](capabilities-contract.md#allocation-and-representation)). A new Time belongs to the creating core and is empty.
- **Root budget (maintainer decision, 2026-10-17).** Kickstart carves one Time on core 0 from the boot Untyped and installs it at `KeySlot::BOOT_TIME` (17). Its budget is infinite, so the root scheduler has an unlimited allowance to give out. There is no finite boot-budget build.
- **Transfer `0`.** Invoked on the source Time: `x2` destination Time key, `x3` amount, `x4..x7` zero. Both capabilities need `WRITE`. A finite source loses the amount; an infinite source stays infinite. An amount of `u64::MAX` makes the destination infinite and is only allowed from an infinite source. An infinite destination stays infinite. Errors: `InvalidOperation` for a zero amount, a nonzero reserved word, the same Time as both operands, Times of different cores, or a finite destination sum reaching `u64::MAX`; `INSUFFICIENT_BUDGET` (with the source's remaining budget) when a finite source holds less than the amount. Every failure leaves both Times unchanged. Success returns zeros.
- **Merge `1`.** Invoked on the destination Time: `x2` source Time key, `x3..x7` zero. Both capabilities need `WRITE`. Moves the source's whole budget into the destination (an infinite source makes the destination infinite) and leaves the source empty. Deleting the source is a separate KeyTable Delete. Errors as for Transfer, except that no amount is checked. Success returns zeros.
- **Query `2`.** No arguments; `READ`. Returns `x1` the remaining budget and `x2` zero.

Implementation status: Time objects, the operations and the boot Time follow this schema.

### Threads and the root Scheduler (maintainer decisions, 2026-10-11)

A Scheduler capability authorizes scheduling. Only the root scheduler configures and starts Threads; it may then move them to child Schedulers.

- **Thread creation.** As in seL4, `Untyped.Retype` creates a Thread from memory alone (see [allocation](capabilities-contract.md#allocation-and-representation)), and it is inert: no AddressSpace, no execution state, never runnable. Only the root scheduler makes it a Thread that can run, by configuring and starting it, which binds it to the root scheduler; Composite's root-only thread creation is this activation step.
- **Schedulers.** Kickstart creates the root Scheduler and installs it at `KeySlot::ROOT_SCHEDULER` (18). Further Schedulers come from `Untyped.Retype`, inert (no parent, Threads or dispatcher), and join the tree through `Adopt`.
- **ConfigureThread.** `Scheduler.ConfigureThread` (`1`), invoked on the root Scheduler with `WRITE`: `x2` Thread key (`WRITE`), `x3` AddressSpace key (`MAP`), `x4` entry PC, `x5` stack SP, `x6` the Thread's own Time key (`WRITE`), `x7` the argument delivered in `x0`. The Thread must be inert (a configured Thread cannot be configured again); the AddressSpace needs a translation root (`NOT_MAPPED`) and an ASID (`NO_ASID_ASSIGNED`); PC and SP must lie in the user range, PC 4-byte and SP 16-byte aligned; the Time must be on this core; other violations fail with `INVALID_OPERATION`. The Thread will start at EL0, bound to the root scheduler.
- **Time binding.** The Thread records its Time's carved identity as kernel bookkeeping, as seL4's TCB records its SchedContext; the binding is not a key, because a Thread migrates between AddressSpaces and owns no KeyTable. The Time key is the authority to make the binding, and the caller keeps it; a retired Time fails the identity check and leaves the Thread unbound.
- **Bound Times are the Thread's own.** ConfigureThread refuses a Time already bound to a live Thread, and while its Thread is live no Transfer or Merge takes budget out of a bound Time (budget can still be transferred or merged into it); both refusals are `INVALID_OPERATION`. Once the Thread is retired the binding no longer validates and the Time can be drained or bound again. A scheduler therefore gives budget from Times that are not bound to a Thread.
- **StartThread.** `Scheduler.StartThread` (`2`): `x2` Thread key; `WRITE` on the Scheduler and the Thread; the Thread must be configured and not yet started, and then becomes runnable. During boot, scheduling begins only after Kickstart establishes the root scheduler; there is no pre-scheduler kernel fallback.

### Scheduler-shared records (maintainer decision, 2026-10-17)

Scheduling records live in pages owned by a user-space scheduler and explicitly shared with the kernel through `Scheduler.ShareRegion`. The scheduler must map the Frame into its own AddressSpace before sharing it; the kernel accesses the shared backing through its kernel mapping. Visibility is limited to schedulers that receive the shared pages; there is no global view or export of the records.

The scheduler may write any bytes in pages it maps, including all scheduling-record fields. Kernel-only TCB/execution state is not in those pages and remains inaccessible to the scheduler. A record's lifetime does not outlive the Thread incarnation it identifies.

Items marked *trial* are numbers and shapes chosen to try, to be revisited after use.

- **Publication only.** Records are the nucleus's publication channel to the scheduler. The nucleus never reads a record back; the authoritative state stays in the kernel-private Thread and Time objects, so a scheduler that corrupts its records only misleads itself. Reconciling scheduler writes with kernel state is not done.
- **Table.** A Scheduler's table is a set of regions; each `ShareRegion` call (`0`) adds one Frame. `x2` Frame key (`READ|WRITE`), `x3` the first slot number the Frame covers, `x4` the stride in bytes, `x5..x7` zero; `WRITE` on the Scheduler. The Frame must be RAM and mapped (`NOT_MAPPED` otherwise). The first call fixes the *record AddressSpace*, the AddressSpace the Frame is mapped in, and the stride; later Frames must be mapped in the same AddressSpace and use the same stride. The stride is a multiple of 64, at least 128 and at most the Frame size. The Frame covers `frame_bytes / stride` slots from `x3`; the first region starts at slot 0, and ranges never overlap. A violation, or sharing a Frame again, fails with `INVALID_OPERATION` and changes nothing. The nucleus initializes slot 0 when the first region is shared and each Thread slot when `AssignRecord` links a Thread to it; the rest of a slot is the scheduler's. *Trial:* at most 8 regions, held in the carved Scheduler object, so the nucleus allocates nothing.
- **Records.** Slot 0 is the Scheduler record and every other slot a Thread record. Each is 128 bytes, 64-byte aligned and written by the nucleus only; fixed-width little-endian fields, with size and offset assertions in the shared ABI. Scheduler record: `sequence` u64 at 0, `layout` u32 (1) and `record_size` u32 at 8, `events` u64 at 16 (events published, equal to the bound EventCount's value), `timer_expirations` u64 at 24, `dispatcher_state` u32 and `flags` u32 at 32, `interrupt_ns` u64 at 40 (the total time the nucleus has spent handling interrupts, rounded up; refreshed with every publication and at each interrupt entry, and not an event), `grant_end_ns` u64 at 48 and `idle_ns` u64 at 56 (the total time the nucleus had nothing to run and waited for an interrupt, rounded up, not counting the interrupts handled meanwhile; published when the wait ends, and not an event), the rest zero. Thread record: `sequence` u64 at 0, `generation` u64 at 8 (the Thread's state-change count), `incarnation` u32 and `state` u32 at 16 (`ThreadState`), `flags` u32 and `block_reason` u32 at 24, `last_stop` u32 and `cpu` u32 at 32, `run_time_ns` u64 at 40 (rounded up), `last_run_ns` u64 at 48, `budget_remaining_ns` u64 at 56 (rounded down), the rest zero. Flag bit 0 is `AWAITING_BUDGET` (runnable, but its Time is exhausted) and bit 1 `HAS_TIME`; without a bound Time `budget_remaining_ns` is `u64::MAX`. The nucleus refreshes the counters only when it publishes a state change; `Time.Query` gives the exact budget.
- **FP/SIMD counters (maintainer decision, 2026-11-19, F10(b)).** The Thread record's words from 64 carry the Thread's FP statistics, as wrapping u64 counters: `fp_entries` at 64 (entries into an FP frame), `fp_saves` at 72 (full saves, to the Thread or into a fault frame), `fp_loads` at 80 (full loads), `fp_zeroes` at 88 (entries with the register file zeroed), `preserved_saves` at 96 (`q8..q15`, FPCR and FPSR saved to a call record), `first_use_traps` at 104, `decays` at 112 and `owner_hits` at 120 (the last three zero until the less eager variant exists). The root Scheduler's record carries the nucleus-wide totals from 64: `thread_switches` at 64, `fp_thread_switches` at 72 (switches with an FP frame on either side) and `fp_entry_toggles` at 80 (with the entry guard), the rest zero; other Schedulers' records keep those words zero. The record size, stride and `layout` (1) stay as they are. The FP counters are published with the Thread's other counters when it is switched away from. They are scheduling information: the root scheduler, or any scheduler holding the record, may read them and decide on them, leaving room for an adaptive FP policy. Implementation status: implemented; `fp-test` and `fp-bench` read them.
- **Publication.** The nucleus is the single writer, under the kernel lock. It stores `sequence + 1` (odd), writes the fields, then stores `sequence + 2` with release ordering. A reader loads `sequence` with acquire ordering, retries while it is odd, reads the fields and retries if `sequence` changed. `sequence` only detects torn reads; `generation` is the kernel-private state-change count that stale-decision checks use.
- **Thread to slot.** `Scheduler.AssignRecord` (`3`): `x2` Thread key (`WRITE`), `x3` slot (at least 1, inside a shared region), `x4..x7` zero; `WRITE` on the Scheduler. The nucleus stores the scheduler and slot in the Thread and publishes its record at once. It does not enforce one Thread per slot: the page is the scheduler's own memory, and `incarnation` lets the scheduler notice reuse. *Trial.* A Thread with no assigned slot has no record; Kickstart starts boot-time Threads before the root dispatcher exists, and the dispatcher adopts them afterwards.
- **Notification.** `Scheduler.BindEventCount` (`4`): `x2` EventCount key (`SEND`), `x3..x7` zero; `WRITE` on the Scheduler. Binding is once-only. After publishing a change to a Thread's state, flags, block reason or stop reason, the nucleus advances the EventCount by one with release ordering; a counter-only refresh does not advance it. A retired EventCount ends the binding.
- **No event ring.** The first ABI has no queue of events. What cannot be rebuilt from the current state is carried by the Thread record's `last_stop` and `last_run_ns` and the Scheduler record's monotonic `events` and `timer_expirations`.
- **Retirement and reuse.** Retiring a Thread publishes it as `Dying` with its incarnation, advances the EventCount, and drops its slot; the scheduler reuses the slot after it sees `Dying`. The nucleus validates a region's Frame identity before each write. Until Frames have a retirement identity, a shared Frame stays valid because the nucleus does not reclaim Frame memory.

Rules for the record ABI:

- Fixed-width fields, explicit layout/alignment, mandatory size/offset assertions, and one shared stride/page-capacity definition.
- Mapping availability and record lifetime are established before safe userspace access; absent pages must not be dereferenced.
- Publication ordering is documented. Release/acquire can publish preceding writes but does not create a coherent snapshot across repeated updates; distinguish independently observed counters from fields requiring a versioned snapshot or equivalent protocol.
- Identity and other non-atomic fields cannot be rewritten concurrently with readers without a sound publication/reuse scheme.
- Units and counter meanings are explicit. Time accounting is in nanoseconds.
- Event summaries have a defined relationship to slots, notifications, and consumption; they are not a second uncoordinated event-delivery mechanism.

Record initialization at Thread start beyond the first publication is not specified.

# Background: design discussion (not specification)

> **Discussion, not rules.** This is the design discussion that led to the decisions above. It argues for options that were later rejected or never chosen, such as kernel priorities and other ways to hand out Time. Nothing here is approved unless the Decisions section says so; where the two differ, the Decisions section wins. Do not cite this part as a rule.

# Discussion: designing a policy-free scheduling subsystem for a microkernel

Every section below is discussion. The sections whose proposals the Decisions changed or did not adopt carry a note saying what was decided.

I think your proposed separation between **`Time`** and **`Scheduler`** is an excellent starting point, and I'd take it somewhat further than seL4 MCS.

The central architectural idea I'd explore is:

> **The kernel controls who is permitted to execute, for how long, and on which CPU. Userspace controls which permitted thread should execute next and when its execution allowance should be renewed.**

The challenge is deciding how much mechanism the kernel needs to make this efficient without accidentally hardcoding one particular scheduling policy.

My preferred design would have:

1. **`Time`** — a transferable, accounted CPU-time resource.
2. **`Scheduler`** — authority to select threads and grant them execution intervals.
3. **`Thread`** — execution state, independent of both.
4. **A minimal kernel dispatcher** — enforces execution limits and hands control back to userspace.
5. **Scheduling events** — asynchronous notifications of blocking, waking, exhaustion, and other relevant state changes.

I would initially avoid kernel-level priorities, periodic replenishment, virtual deadlines, fairness accounting, and scheduling classes.

Those are policies, and exposing them through the core kernel API would unnecessarily constrain what userspace can implement.

There are, however, a few subtle exceptions.

In particular, **CPU budget enforcement, scheduler execution guarantees, and IPC budget propagation can't be delegated entirely to userspace** without undermining performance or isolation.

Let's build the design from first principles.

---

# Discussion: 1. What should the kernel actually guarantee?

I'd start with a fairly strict list of kernel responsibilities.

### Mechanisms the kernel must provide

| Mechanism | Why it belongs in the kernel |
|---|---|
| CPU execution accounting | Only the kernel can reliably account for execution |
| Execution preemption | Userspace cannot preempt an uncooperative running thread |
| Capability enforcement | Prevents unauthorized scheduling or budget allocation |
| Thread state transitions | Blocking, waking, and termination involve kernel state |
| Scheduler activation | Something must run when a thread blocks or exhausts its allowance |
| Timer programming | Needed to preempt threads at precise budget boundaries |
| CPU ownership | Prevents conflicting scheduling decisions |
| IPC budget transfer | Must be atomic with relevant execution-state transitions |
| Multicore dispatch | Needs synchronization with CPU execution state |

### Policies I'd keep outside the kernel

| Policy | Userspace implementation |
|---|---|
| Fixed-priority scheduling | Priority queues |
| Round-robin | Queue rotation and timeslices |
| MuQSS-style scheduling | Virtual deadlines and slice accounting |
| CFS/EEVDF | Virtual runtime, lag, virtual deadlines |
| EDF | Absolute deadline queues |
| Rate-monotonic scheduling | Static priorities derived from periods |
| Sporadic servers | Budget accounting and replenishment |
| CPU shares | Hierarchical weighted allocation |
| Priority inheritance | Dependency tracking and priority propagation |
| Work stealing | Scheduler-to-scheduler migration |
| CPU affinity preferences | Scheduler policy |
| Load balancing | Scheduler policy |

Notice that I'm distinguishing **enforcing a CPU execution limit** from **deciding how CPU time is replenished**.

This distinction is important.

A kernel may need to enforce a limit of 2 ms, but it doesn't necessarily need to know whether those 2 ms represent:

- A round-robin quantum.
- A sporadic-server budget.
- An EDF reservation.
- An interactive MuQSS slice.
- A hierarchical CPU allocation.
- A temporary budget loan from another thread.

These should ideally look identical to the kernel.

---

# Discussion: 2. The three main objects

Here's the object model I'd explore.

```text
                  ┌─────────────────┐
                  │    Scheduler    │
                  │                 │
                  │ Dispatch policy │
                  │ Thread control  │
                  └────────┬────────┘
                           │
                     dispatch()
                           │
                           ▼
                  ┌─────────────────┐
                  │     Thread      │
                  │                 │
                  │ Registers       │
                  │ Execution state │
                  └────────┬────────┘
                           │
                     executes with
                           │
                           ▼
                  ┌─────────────────┐
                  │      Time       │
                  │                 │
                  │ Remaining       │
                  │ Consumed        │
                  │ Accounting      │
                  └─────────────────┘
```

The three objects have independent lifetimes.

A thread doesn't intrinsically own its CPU budget.

A budget doesn't intrinsically belong to a particular thread.

And a scheduler doesn't necessarily own the CPU time that it schedules.

That last distinction is especially important for hierarchical scheduling.

## 2.1 `Thread`: execution state

A thread contains only the state needed to execute it.

Conceptually:

```rust
struct Thread {
    registers: Registers,
    address_space: AddressSpace,
    ipc_state: IpcState,
    exception_handler: Option<Endpoint>,

    state: ThreadState,
    scheduler: SchedulerId, // the important bit
}
```

No permanent priority.

No permanent quantum.

No virtual runtime.

No CPU budget.

The kernel needs to know which scheduler is responsible for the thread, but that relationship should be changeable through an authorized operation.

A thread might have states such as:

```rust
enum ThreadState {
    Runnable,
    Running,
    Blocked,
    Suspended,
}
```

I'd avoid storing `BudgetExhausted` as a thread state.

Budget exhaustion should be independent of whether the thread is logically runnable.

A runnable thread can simply lack permission to execute until its scheduler provides more time.

That distinction becomes useful for EDF, sporadic servers, and hierarchical scheduling.

## 2.2 `Time`: execution authority

I'd define `Time` as a resource representing an amount of CPU execution time that may be consumed.

A very simple starting point:

```rust
struct Time {
    remaining: Duration,
    consumed: Duration,
}
```

But this raises a fundamental question:

**Where does the time come from?**

If arbitrary userspace can increase `remaining`, then the object isn't actually providing resource isolation.

Consequently, `Time` needs either an accounting hierarchy or a separate replenishment authority.

We'll return to this shortly.

## 2.3 `Scheduler`: scheduling authority

A `Scheduler` object establishes authority over a collection of threads.

Conceptually:

```rust
struct Scheduler {
    threads: Set<ThreadId>,
    event_queue: Queue<SchedulingEvent>,
    cpu_set: CpuSet,
}
```

The scheduler's userspace implementation can maintain whatever data structures it wants.

For example:

```rust
struct MuqssScheduler {
    queues: [SkipList; CPU_COUNT],
}

struct EdfScheduler {
    deadlines: BinaryHeap<ThreadId>,
}

struct RoundRobinScheduler {
    runnable: VecDeque<ThreadId>,
}
```

The kernel doesn't need to know which algorithm is being used.

The `Scheduler` kernel object should primarily provide authorization, state tracking, and event delivery.

---

# Discussion: 3. The most important primitive: `dispatch`

> **Decided differently in part** ([Decisions](#decisions)): `Scheduler.Dispatch` (op `6`) is invoked by the dispatcher only, blocks it, and runs a Thread on the Thread's own bound Time; dispatching any other Time is later work.

Rather than exposing kernel operations such as:

```text
set_priority()
set_deadline()
set_timeslice()
set_policy()
```

I'd expose a general operation:

```rust
Scheduler::dispatch(
    thread: ThreadCap,
    time: TimeCap,
    duration: Duration,
)
```

Semantically:

> Execute this thread, using the specified time resource, for at most the specified duration.

For example:

```rust
scheduler.dispatch(
    thread_a,
    time_a,
    Duration::from_millis(2),
);
```

The kernel verifies that:

1. The caller has scheduling authority over `thread_a`.
2. The time resource is valid and spendable.
3. The requested duration doesn't exceed the available allowance.
4. The thread is runnable.
5. The requested CPU is authorized.
6. The thread isn't already executing elsewhere.

If valid, it installs the execution state.

Conceptually:

```text
CPU 0
  │
  ▼
Scheduler
  │
  │ dispatch(A, TimeA, 2 ms)
  ▼
Thread A executes
  │
  ├── blocks after 0.8 ms
  │       → return control
  │       → charge 0.8 ms
  │
  └── exhausts 2 ms
          → kernel preempts A
          → return control
          → charge 2 ms
```

The remaining time stays in the `Time` object.

The scheduler can then decide what to do.

### Why this is powerful

This one primitive is sufficient to express many scheduling policies.

**Round-robin:**

```rust
loop {
    let thread = queue.pop_front();

    dispatch(thread, thread.time, QUANTUM);

    queue.push_back(thread);
}
```

**Fixed priority:**

```rust
loop {
    let thread = highest_priority_runnable();

    dispatch(thread, thread.time, QUANTUM);
}
```

**EDF:**

```rust
loop {
    let thread = earliest_deadline_runnable();

    dispatch(thread, thread.time, quantum_until_next_event());
}
```

**MuQSS:**

```rust
loop {
    let thread = earliest_virtual_deadline();

    dispatch(thread, thread.time, thread.remaining_slice);

    update_virtual_deadline(thread);
}
```

**Weighted fair scheduling:**

```rust
loop {
    let thread = select_eligible_thread();

    let slice = calculate_fair_slice(thread);

    dispatch(thread, thread.time, slice);

    update_service_accounting(thread);
}
```

In all cases, the kernel sees the same operation.

### One necessary refinement: dispatch must support interruption

The examples above are deliberately simplified.

A real `dispatch()` cannot simply run a thread for 2 ms and then return.

Suppose a higher-priority thread becomes runnable after 100 μs.

The scheduler needs an opportunity to replace the currently executing thread.

Therefore, I'd make the execution interval terminate on:

- Budget exhaustion.
- Thread blocking.
- Explicit yielding.
- A scheduler-requested preemption.
- Selected scheduler-relevant events.
- Thread termination.

That leads to the next important component.

---

# Discussion: 4. Scheduler activations: how userspace regains control

> **Decided** ([Decisions](#decisions), "State, notification and activation"): state, notification and activation are separate mechanisms; `replace` is parked until multicore ([open questions](scheduling-decisions.md#d5-scheduler-hierarchy-and-multicore)).

I think this is the most important part of the interface after `dispatch()`.

A scheduler needs to know when a decision is necessary.

It also needs to preempt a running thread when something important happens.

I'd introduce a small set of scheduling events:

```rust
enum SchedulingEvent {
    Runnable(ThreadId),
    Blocked(ThreadId),
    Yielded(ThreadId, Duration),
    Preempted(ThreadId, Duration),
    BudgetExhausted(ThreadId),
    Exited(ThreadId),
}
```

These are illustrative; they needn't all be independent event types in the actual ABI.

The kernel would deliver events through a scheduler-specific notification channel.

### Example: a higher-priority thread wakes

Suppose:

```text
CPU 0:
    A running (priority 10)

Scheduler:
    B blocked (priority 20)
```

B receives an IPC message and becomes runnable.

```text
                    IPC arrives
                         │
                         ▼
                  Kernel wakes B
                         │
                         ▼
                  Notify Scheduler
                         │
                         ▼
                   Scheduler runs
                         │
                 Compare priorities
                         │
                         ▼
                  Preempt A, run B
```

The kernel doesn't need to understand priorities.

It only needs to notify the scheduler that a thread's state has changed.

The scheduler decides whether preemption is necessary.

## 4.1 The scheduler must be able to preempt immediately

A naive implementation would only invoke the scheduler when the current thread's quantum expires.

That's insufficient for responsive scheduling.

If A has 10 ms remaining but B is a latency-critical thread, B might need to run immediately.

I'd support something like:

```rust
Scheduler::preempt(cpu);
```

Or, preferably, combine preemption and dispatch:

```rust
Scheduler::replace(cpu, thread_b, time_b, duration);
```

This avoids a separate preemption followed by a second dispatch.

Internally, the operation atomically:

1. Stops accounting execution to A.
2. Records A's remaining allowance.
3. Accounts for the context-switch boundary.
4. Installs B's execution context.
5. Arms the next execution timer.

On another core, it may require an IPI.

### Why I prefer atomic replacement

A two-step interface:

```text
preempt(A)
dispatch(B)
```

introduces an interval in which no useful work is selected.

More importantly, it creates race conditions between the operations.

An atomic replacement allows the kernel to validate the expected execution state and commit the scheduling decision as one transaction.

This is closely related to the approach used by Google's **ghOSt**, which delegates scheduling policy to userspace and lets agents commit scheduling decisions through a transactional kernel interface. Its implementations demonstrate that userspace schedulers can support EDF, CFS-like scheduling, centralized FIFO, and other policies with a common mechanism. [Google Research](https://research.google/pubs/ghost-fast-and-flexible-user-space-delegation-of-linux-scheduling/?utm_source=chatgpt.com)

## 4.2 Avoiding stale scheduling decisions

Consider:

```text
Scheduler reads:
    B is runnable

Before dispatch:
    B blocks on another CPU
```

Or:

```text
Scheduler decides:
    Replace A with B

Meanwhile:
    A blocks on an IPC endpoint
```

I'd attach a generation counter to scheduling state.

For example:

```rust
struct DispatchRequest {
    cpu: CpuId,
    expected_generation: u64,
    thread: ThreadCap,
    time: TimeCap,
    duration: Duration,
}
```

The kernel accepts the request only if the relevant state still matches.

If not, it rejects the operation and returns current state.

This permits optimistic scheduling decisions without holding kernel locks while a userspace scheduler performs potentially complicated computations.

---

# Discussion: 5. The difficult question: who can create or replenish `Time`?

> **Decided** ([Decisions](#selected-model-maintainer-decision-2026-10-17)): a Time holds only a remaining budget; budget moves by permanent Transfer or Merge, with no loan, no parent link and no automatic return; Retype creates an empty Time; the root budget is the infinite boot Time; the nucleus never replenishes, so replenishment is scheduler code.

This is where I'd consider two alternative architectures.

They have substantially different properties.

## Design A: `Time` is a consumable execution allowance

In this design:

```rust
struct Time {
    remaining: Duration,
    consumed: Duration,
}
```

A scheduler can grant execution only while the object has remaining time.

Time must be obtained from an authority that can allocate it.

For example:

```text
Root CPU authority
        │
        ▼
Root Scheduler
        │
        ├── Time A: 2 ms
        ├── Time B: 5 ms
        └── Time C: 3 ms
```

The root scheduler can replenish these objects according to any algorithm.

This permits ordinary round-robin scheduling, EDF, sporadic servers, and so on.

However, there's an important problem.

**If the scheduler replenishes time freely, the budgets are not enforced resource limits against that scheduler.**

A scheduler could repeatedly grant 2 ms to A, effectively allowing A to consume unlimited CPU time.

That might be fine for the root scheduler, but not for an untrusted child scheduler.

The solution is hierarchical CPU-time authority.

## 5.1 Hierarchical budget allocation

Imagine:

```text
                      CPU 0
                        │
                        ▼
                 Root Scheduler
                        │
               100% CPU authority
                        │
          ┌─────────────┴─────────────┐
          │                           │
          ▼                           ▼
     Scheduler A                  Scheduler B
     Budget: 30%                 Budget: 70%
          │                           │
     ┌────┴────┐                 ┌────┴────┐
     ▼         ▼                 ▼         ▼
   Thread 1  Thread 2          Thread 3  Thread 4
```

Scheduler A can implement MuQSS.

Scheduler B can implement EDF.

The root scheduler doesn't need to know anything about either policy.

But the kernel must guarantee that A cannot use more CPU time than its parent authorizes.

A possible object model:

```rust
struct Time {
    balance: Duration,
    parent: Option<TimeId>,
}
```

Time can be transferred or subdivided.

For example:

```rust
root_time.transfer(child_time, 2.ms());
```

The crucial invariant is:

\[
\sum \text{outstanding delegated time}
+
\text{locally available time}
\leq
\text{authorized capacity}
\]

This is a conservation rule for *already funded* execution time, not an attempt to represent all future CPU capacity as a finite quantity.

There is also a potential complication: if a parent scheduler allocates its entire current allowance, the parent may not have any CPU time left to execute itself.

That leads to scheduler execution guarantees, which we'll examine shortly.

### Advantages

- Natural capability-based resource delegation.
- No arbitrary CPU usage creation by untrusted schedulers.
- Strong foundation for hierarchical scheduling.
- Easy to reason about accounting ownership.

### Disadvantages

- Requires mechanisms for transferring and reclaiming time.
- Requires decisions about what happens to unspent time.
- Adds complexity to the critical scheduling path.
- Reservation semantics still need to be implemented somewhere.

Most importantly, time conservation alone doesn't specify *when* an allocation becomes available.

Giving a child 20 ms doesn't say whether it may consume those 20 ms immediately or only over the next 100 ms.

That distinction matters for temporal isolation.

---

# Discussion: 6. Design B: `Time` is an accounting object, while scheduling authority controls execution

> **Not adopted** ([Decisions](#selected-model-maintainer-decision-2026-10-17)): a Time is a budget the nucleus enforces, not only an accounting context.

This is a different approach, and I think it's worth considering seriously.

Instead of treating `Time` primarily as a resource that must be consumed and replenished, treat it as an **execution accounting context**.

```rust
struct Time {
    consumed_total: Duration,
    execution_limit: Option<Duration>,
}
```

A scheduler dispatches a thread with:

```rust
dispatch(thread, time, duration);
```

The kernel charges execution against `time` and enforces `duration`.

But the scheduler is responsible for deciding whether that thread should execute again.

In this architecture, the ability to dispatch a thread is itself the relevant authority.

An untrusted scheduler is constrained because its parent controls how much execution time the scheduler's domain receives.

Consider:

```text
Root Scheduler
      │
      │ grants execution to domain A
      ▼
Scheduler A
      │
      ├── Thread A1
      ├── Thread A2
      └── Thread A3
```

The root grants A an execution interval of 3 ms.

During that interval, Scheduler A can allocate CPU time however it wants.

It cannot exceed its parent's 3 ms allowance.

The kernel charges execution against both the current thread's accounting object and its enclosing domain's budget.

### Why this is attractive

The kernel doesn't need to implement periodic replenishment.

The root scheduler can implement an EDF server, sporadic server, or another policy.

The child scheduler can independently implement round-robin, MuQSS, or something else.

The kernel merely enforces the maximum execution allowance at each active scheduling level.

Conceptually:

```text
CPU execution
      │
      ├── Charge Thread's Time
      │
      ├── Charge Scheduler A's Time
      │
      └── Charge Root's Time
```

This introduces hierarchical accounting but avoids requiring each thread to possess a separately funded CPU-time allocation.

### The main drawback

Naively charging every level introduces an O(depth) operation for each accounting update.

Also, a scheduler cannot meaningfully execute its children without receiving execution capacity from its parent.

Those problems can be addressed with an *effective execution limit*.

For example:

```text
Current thread allowance:    5 ms
Parent allowance:            3 ms
Grandparent allowance:      10 ms
```

The next execution interval is limited to:

\[
\Delta t = \min(5,3,10)=3\text{ ms}
\]

The kernel only needs to program a timer for the nearest limit.

It can defer some accounting operations until a context switch, rather than updating every accounting object on every timer tick.

For very deep scheduling hierarchies, some per-level accounting is still unavoidable if every level requires exact usage information. Depth limits, cached effective deadlines, and lazy accounting would help.

**My preference would be a hybrid of Designs A and B:** independently identifiable `Time` accounting objects, with explicit authority to fund or limit execution and an optional parent-controlled scheduling domain.

This permits both client-funded RPC and hierarchical scheduling without making every timeslice a newly allocated capability.

---

# Discussion: 7. Scheduler hierarchies: the kernel should support them, but not prescribe them

> **Decided** ([Decisions](#decisions), "Child schedulers"): a strict tree at most 4 levels deep, built with `Adopt`, with the parent's dispatch limit as the child's grant; the donation schema is open ([open questions](scheduling-decisions.md#d5-scheduler-hierarchy-and-multicore)).

I would make `Scheduler` objects nestable.

However, I wouldn't necessarily model a scheduler as a thread that repeatedly receives CPU time and then manually yields to its children.

That can introduce unnecessary context switches.

Instead, think of the scheduler as a **domain of scheduling authority**, which happens to be managed by a userspace thread.

```text
                    Root Scheduler
                          │
              ┌───────────┴───────────┐
              │                       │
         Scheduler A              Scheduler B
              │                       │
       EDF policy                  MuQSS policy
              │                       │
        ┌─────┴─────┐           ┌─────┴─────┐
        │           │           │           │
      Thread 1    Thread 2    Thread 3    Thread 4
```

A useful capability distinction emerges:

- A `Scheduler` capability grants authority over a scheduling domain.
- A `Time` capability grants access to a particular accounting or execution resource.
- A `Thread` capability grants authority over the thread itself.

These rights should remain independent.

For example, a filesystem server might receive a client's `Time` authority through IPC without receiving authority to modify the client's scheduler.

Similarly, a scheduler might be authorized to run a thread without being allowed to modify the thread's address space or registers.

## 7.1 A scheduler needs explicit authority over CPUs

A scheduler shouldn't automatically be permitted to execute threads on any CPU.

I'd associate every scheduling domain with an allowed CPU set:

```rust
struct Scheduler {
    allowed_cpus: CpuSet,
    parent: Option<SchedulerId>,
}
```

A child scheduler's CPU authority must be a subset of its parent's.

One possibility is to make CPU scheduling authority a capability in its own right, but I wouldn't introduce that object immediately.

It might be sufficient for a `Scheduler` capability to carry CPU-set restrictions.

For example:

```text
Scheduler A:
    CPUs 0–3

Scheduler B:
    CPUs 2–3
```

This supports overlapping scheduling domains provided their execution grants are coordinated through their common parent.

On a particular CPU, the kernel must still enforce exclusive ownership of the active dispatch slot.

## 7.2 Don't require a context switch at every hierarchy level

Suppose:

```text
Root Scheduler
    ↓
Scheduler A
    ↓
Scheduler B
    ↓
Thread X
```

A naive implementation might require:

```text
Root runs
    → A runs
        → B runs
            → X runs
```

Then, when X blocks:

```text
X → B → A → Root
```

This is expensive.

I'd prefer allowing a parent to delegate execution authority to a child scheduler such that the child can directly dispatch its own threads.

The root only needs to become involved when:

- The child exhausts its allocated execution allowance.
- The child explicitly yields its allocation.
- The parent revokes the child's execution authority.
- A scheduling event requires the parent's attention.

The kernel can cache the currently effective execution allowance, avoiding repeated parent scheduler activations.

This is where scheduling domains become more useful than simply having scheduler threads call one another.

---

# Discussion: 8. A better `Time` object: separate accounting and allowance

> **Not adopted as written** ([Decisions](#selected-model-maintainer-decision-2026-10-17)): a Time holds only its remaining budget; consumption is published per Thread in the scheduler-shared records (`run_time_ns`), not kept in the Time.

I'd consider structuring the `Time` object approximately like this:

```rust
struct Time {
    consumed: u64,
    available: u64,

    // Internal kernel state
    active_execution: Option<ExecutionId>,
}
```

But I would be careful about the interpretation of `available`.

There are two quantities worth distinguishing:

**Consumption:** How much CPU execution has been charged to this object?

**Allowance:** How much additional execution is currently authorized?

Consumption is monotonically increasing, modulo counter representation.

Allowance can increase through authorized grants and decrease through consumption.

These are logically separate, even if implemented in one object.

### Possible interface

```rust
Time::consumed() -> Duration

Time::remaining() -> Duration

Time::transfer(
    destination: TimeCap,
    amount: Duration,
)

Time::reclaim(
    source: TimeCap,
    amount: Duration,
)
```

Transfers should require the corresponding rights.

An ordinary thread holding a `Time` capability for accounting purposes must not automatically have permission to withdraw or transfer the remaining budget.

I would use attenuated capability rights:

| Right | Meaning |
|---|---|
| Observe | Read consumption and remaining allowance |
| Spend | Authorize execution against the object |
| Transfer | Move available allowance to another object |
| Configure | Change permitted accounting configuration |
| Reclaim | Recover unused delegated allowance, subject to revocation rules |

This is only a conceptual rights model; I wouldn't necessarily implement five separate rights immediately.

The important part is separating *observing consumption* from *controlling execution authority*.

### A subtle security property

When a thread executes using a `Time` object, it should not necessarily gain possession of a capability to that object.

Otherwise, a server executing on a client's donated budget might acquire the ability to transfer or modify that budget.

Instead, the kernel should support something like:

```text
Execution context:
    Thread = FilesystemServer
    Time   = ClientA
```

The server can consume the client's allowance without acquiring management authority over it.

This is another reason to keep `Thread`, `Time`, and `Scheduler` independent.

---

# Discussion: 9. Budget donation: explicit versus implicit

> **Partly decided** ([Decisions](#selected-model-maintainer-decision-2026-10-17)): a protected procedure call keeps the caller's Time, so the client pays for the server's work. Attribution across calls and the scheduler donation schema are open ([open questions](scheduling-decisions.md#d5-scheduler-hierarchy-and-multicore)).

There are at least three designs.

## Option A: automatic donation on synchronous IPC

Similar to seL4 MCS passive servers.

```text
Client A
    |
    | synchronous call
    ▼
Server S

Time A automatically follows execution.
```

Advantages:

- Efficient.
- Natural for synchronous RPC.
- Client CPU accounting follows the request.
- No userspace scheduler round trip is necessary for ordinary IPC.

Disadvantages:

- Kernel IPC semantics become coupled to scheduling.
- Nested calls need explicit unwind rules.
- Server budget exhaustion requires careful handling.
- Asynchronous requests don't fit naturally.

seL4 MCS uses passive servers and reply objects to manage scheduling-context donation along synchronous IPC chains. Its kernel also handles sporadic replenishment, which is one respect in which your proposed architecture could push more policy into userspace. [seL4 Documentation](https://docs.sel4.systems/Tutorials/mcs.html?utm_source=chatgpt.com)

## Option B: explicit donation

Make the scheduling-context transfer explicit:

```rust
ipc.call(
    server,
    message,
    donate_time = client_time,
);
```

Or:

```rust
Time::lend(server_thread);
```

Now donation becomes an independent operation.

Advantages:

- Clearer authority boundaries.
- Applicable beyond conventional RPC.
- The caller can choose whether to donate.
- Works with scheduling-context capabilities already in use.

Disadvantages:

- The kernel still needs to track ownership and restoration.
- More complicated call-site semantics.
- Potentially additional capability manipulation.

## Option C: scheduler-mediated donation

On IPC, the kernel simply reports that the client is waiting for the server.

The scheduler determines whether to transfer time.

```text
A calls S
    ↓
Scheduler notified
    ↓
Scheduler assigns A's Time to S
    ↓
S executes
```

This is arguably the purest mechanism-not-policy design.

Unfortunately, it introduces a userspace scheduling round trip on the critical IPC path.

For a microkernel with frequent synchronous calls, that could be prohibitively expensive.

### My choice: explicit authorization, automatic execution transfer

I'd use a hybrid.

A client explicitly authorizes budget donation as part of a synchronous call.

The kernel then implements the transfer and restoration without invoking the userspace scheduler on every IPC transition.

Something like:

```rust
ipc.call(
    server,
    message,
    IpcFlags::DONATE_TIME,
);
```

The budget travels with the synchronous call chain.

For asynchronous execution, I'd provide a separate explicit mechanism.

The important boundary is that the kernel implements **transfer of an already authorized execution resource**, not any particular policy for deciding how much budget the client should receive.

---

# Discussion: 10. Scheduler activation is itself a scheduling problem

This is the biggest bootstrapping problem.

Suppose CPU 0 is executing thread A.

Thread B becomes runnable.

The scheduler needs to run to decide whether B should preempt A.

But the scheduler itself is a userspace thread.

Who schedules the scheduler?

You cannot solve this by asking the same scheduler to decide when it should execute.

There has to be some mechanism above it.

I see three choices.

## Option A: privileged scheduler activation

The kernel immediately activates the appropriate userspace scheduler when it needs a decision.

Scheduler execution is treated specially.

```text
Application thread
       │
       │ scheduling event
       ▼
Kernel
       │
       ▼
Userspace scheduler
       │
       │ dispatch(...)
       ▼
Application thread
```

This resembles a scheduler activation or upcall mechanism.

The kernel doesn't choose which application thread to run.

It merely ensures that the responsible scheduler receives execution.

The problem is resource accounting.

If scheduler execution is effectively unlimited, a malicious scheduler could monopolize the CPU.

## Option B: scheduler execution consumes its own budget

Give the scheduler a `Time` object.

```text
Scheduler S:
    Time = 1 ms / 10 ms
```

Scheduler activation consumes its own budget.

This is easy to understand, but has a serious drawback.

If the scheduler exhausts its budget, its entire domain can stop making progress.

This may be acceptable for an untrusted child scheduler but not for the root scheduler.

It also introduces awkward interactions between scheduling decisions and replenishment.

## Option C: parent-funded scheduler execution

This is my preferred approach for hierarchical scheduling.

Every child scheduler is scheduled by its parent.

```text
Root Scheduler
      │
      │ funds scheduler execution
      ▼
Child Scheduler
      │
      │ allocates application execution
      ▼
Application Threads
```

The parent controls both:

- CPU time consumed by the child's applications.
- CPU time consumed by the child scheduler itself.

This avoids free scheduler execution.

But at the root, the kernel still needs a privileged activation path.

I'd make the root dispatcher a small, special mechanism rather than pretending that the entire scheduling hierarchy can be implemented without any kernel scheduling rules.

### A proposed kernel invariant

**Every CPU always has a designated controlling scheduler, and the kernel can activate that scheduler when a scheduling decision is required.**

The root scheduler's activation authority is established at boot.

Child schedulers receive restricted activation authority from their parents.

The kernel needs only enough built-in behavior to ensure that the appropriate scheduler can regain control, subject to its execution allowance.

This is analogous to having a minimal trap handler for scheduling decisions rather than a full in-kernel scheduling policy.

---

# Discussion: 11. What about timer management?

> **Decided** ([Decisions](#decisions), "Tickless enforcement", "ArmTimer", "Clock"): a one-shot timer for the earliest of budget exhaustion, the dispatch limit and the next deadline; one activation timer per dispatcher.

I'd keep deadlines and replenishment entirely in userspace, but provide a general timer mechanism.

A scheduler needs to request activation at an absolute time:

```rust
Scheduler::arm_timer(
    deadline: Instant,
);
```

For example, an EDF scheduler can ask to be awakened at its next job release.

A sporadic-server scheduler can request activation at its next replenishment.

A MuQSS implementation can request a scheduling decision at its next relevant virtual deadline.

The kernel doesn't need to know why the timer exists.

There are two distinct timers:

| Timer | Purpose | Controlled by |
|---|---|---|
| Execution timer | Preempt a running thread when its grant expires | Kernel |
| Policy timer | Activate a scheduler at a requested wall-clock time | Userspace |

These should be conceptually independent, even if implemented through the same hardware timer.

For efficiency, the kernel can program the next hardware interrupt as:

\[
t_{\text{interrupt}} =
\min(
t_{\text{execution limit}},
t_{\text{scheduler alarm}}
)
\]

On a multicore system, this calculation is performed for the relevant CPU.

I would make policy timers use absolute monotonic timestamps rather than durations. This simplifies periodic releases, deadline scheduling, and handling delayed activations.

---

# Discussion: 12. Multicore scheduling: centralized and per-CPU schedulers

> **Decided** ([SMP design](smp-design.md#decisions)): per-core root Schedulers, dispatchers and boot Times, and a cross-core `Scheduler.MoveThread`; a Time belongs to one core.

One desirable property is to support both designs without requiring separate kernel interfaces.

### Centralized scheduler

```text
             Scheduler
            /    |    \
           /     |     \
        CPU 0  CPU 1  CPU 2
```

A single userspace scheduler makes decisions for all CPUs.

This is attractive for:

- Global EDF.
- Global MuQSS-like scheduling.
- Small multicore systems.
- Centralized load balancing.

But it can become a contention point.

### Per-CPU schedulers

```text
Scheduler 0     Scheduler 1     Scheduler 2
    |               |               |
  CPU 0           CPU 1           CPU 2
```

Each CPU has an independent scheduler.

They can communicate to migrate threads or steal work.

This is attractive for:

- Scalable fair schedulers.
- CPU-local real-time scheduling.
- Cache-affinity-aware policies.

### Supporting both

I'd let a `Scheduler` capability authorize dispatch operations for a CPU set.

The scheduler can have one or multiple userspace agents.

The kernel doesn't need to distinguish centralized from distributed scheduling policies.

However, a centralized scheduler should be able to issue a batch of dispatch decisions:

```rust
Scheduler::commit(&[
    Dispatch { cpu: 0, thread: a, ... },
    Dispatch { cpu: 1, thread: b, ... },
    Dispatch { cpu: 2, thread: c, ... },
]);
```

Batching reduces syscall overhead and can help coordinate migrations. It doesn't necessarily imply that all remote CPUs change execution simultaneously.

I'd make this an optimization added after the single-CPU path works.

---

# Discussion: 13. The design I'd actually implement

> **A proposal, not the selected design.** What was adopted is the [Decisions](#decisions) section.

After considering the alternatives, I would start with a smaller architecture than the full proposal above.

In particular, I'd resist introducing a complicated transferable time-credit economy immediately.

Instead, I'd implement **execution grants** as the fundamental mechanism.

## Kernel objects

### `Thread`

Holds execution state and a reference to its scheduling domain.

### `Time`

Holds:

```rust
struct Time {
    consumed: u64,
    remaining: u64,
}
```

The object can be used independently of any particular thread.

Execution charges can be attributed to it, and its allowance can be modified only through authorized operations.

### `Scheduler`

Defines a scheduling domain.

Holds:

```rust
struct Scheduler {
    controlling_agent: ThreadId,
    parent: Option<SchedulerId>,
    allowed_cpus: CpuSet,
    event_queue: EventQueue,
}
```

No priorities, deadline trees, run queues, or fairness data structures need to live in the kernel.

### `ExecutionGrant`

I would initially make this **transient kernel state**, not a separate capability object.

```rust
struct ExecutionGrant {
    thread: ThreadId,
    time: TimeId,
    scheduler: SchedulerId,
    cpu: CpuId,
    limit: Duration,
}
```

The grant exists while a thread is executing, and the kernel accounts CPU consumption against its `Time`.

The implementation could simply store this in per-CPU state.

I would not expose a separate `ExecutionGrant` capability unless a real use case emerges for transferring outstanding grants independently of their associated `Time` objects.

## Minimal operations

I think the initial ABI could be surprisingly small.

| Operation | Purpose |
|---|---|
| `Scheduler::attach(thread)` | Assign thread to scheduler domain |
| `Scheduler::detach(thread)` | Remove scheduling authority |
| `Scheduler::dispatch(thread, time, limit)` | Execute a thread |
| `Scheduler::replace(cpu, thread, time, limit)` | Atomically replace current execution |
| `Scheduler::wait()` | Receive scheduling events |
| `Scheduler::arm_timer(deadline)` | Request timed activation |
| `Time::read()` | Observe consumption and allowance |
| `Time::grant(amount)` | Add allowance using authorized funding |
| `Time::transfer(destination, amount)` | Transfer funded allowance |

The `Time::grant` operation needs a precise authority model: it must not allow an untrusted scheduler to manufacture additional execution capacity. For the first implementation, I'd restrict it to the root scheduling authority or to transferring capacity from an existing parent allocation.

For hierarchical schedulers, add delegation of scheduling authority and effective execution limits. You needn't implement a fully general hierarchy on day one, but the capability model should not prevent it.

### One very useful additional mechanism: direct handoff

I'd strongly consider:

```rust
Scheduler::handoff(
    from: ThreadCap,
    to: ThreadCap,
    time: TimeCap,
    limit: Duration,
)
```

This would atomically suspend or block one thread and execute another using the specified time resource.

Ordinary synchronous IPC could use a specialized fast path for this.

It would support:

- Client-funded RPC.
- Coroutine-style thread handoff.
- Work delegation.
- Fast userspace scheduling transitions.

A general scheduler could authorize the handoff in advance, rather than being invoked on every RPC.

---

# Discussion: 14. Some important traps to avoid

There are a few architectural issues I would address early.

### 14.1 Don't make scheduler events lossy

If a scheduler misses a `Runnable` notification, a thread might never execute again.

Events must either be reliably delivered or accompanied by authoritative state that the scheduler can inspect.

I particularly like ghOSt's approach of combining event delivery with kernel-maintained status information and sequence numbers.

A useful design is:

```rust
struct ThreadStatus {
    generation: u64,
    state: ThreadState,
    consumed: u64,
}
```

A shared-memory status page could expose this without requiring a syscall for every accounting query.

The kernel would publish state transitions with appropriate memory ordering. Sequence numbers let the scheduler detect stale observations.

You still need an overflow protocol for event queues, but lost notifications no longer have to mean lost thread state.

### 14.2 Make scheduler operations generation-safe

A thread might block, wake, migrate, or terminate while a scheduling decision is being prepared.

A scheduler should never accidentally dispatch a thread based on stale state.

Use generation counters and atomic validation at dispatch time.

### 14.3 Define what happens when a scheduler crashes

Suppose a scheduler controls 100 threads and terminates unexpectedly.

Possible recovery policies include:

- Suspend all controlled threads.
- Return scheduling control to the parent.
- Activate a designated replacement scheduler.

I'd implement the first two mechanisms in the kernel.

The parent can decide whether to restart the scheduler, migrate its threads, or terminate its domain.

A crashed root scheduler is a separate failure: no purely userspace policy can recover unless the kernel already has a trusted emergency activation or replacement mechanism.

### 14.4 Don't confuse available CPU time with guaranteed service

A scheduler may have permission to consume 5 ms without being guaranteed 5 ms of actual execution within a particular interval.

To express guarantees, the parent must also provide a temporal service contract.

For example:

```text
Child scheduler:
    5 ms execution capacity per 10 ms interval
```

A child with an ordinary allowance of 5 ms cannot infer that guarantee merely from `Time::remaining() == 5 ms`.

This matters if you eventually want compositional real-time scheduling.

### 14.5 Account for kernel execution time

You should explicitly define whether CPU time spent executing syscalls, handling IPC, and performing kernel work is charged to the currently active `Time`.

I'd generally charge synchronous kernel work to the active execution context.

Otherwise, an application could consume CPU without exhausting its allowance by generating excessive kernel activity.

Interrupt and deferred-work accounting is harder.

A device interrupt may not be attributable to a particular client. Kernel mechanisms should at least distinguish such execution, even if the attribution policy is initially simple.

### 14.6 Time exhaustion during synchronous IPC

Consider:

```text
Client A
   ↓
Filesystem Server
   ↓
Device Driver
```

All three execute using A's time resource.

What happens if the budget expires inside the device driver?

One possible semantic is to suspend the entire execution chain until A's allowance is replenished.

But that can cause priority inversion or block unrelated clients if shared state is held.

I'd distinguish between a normal exhaustion notification and a terminal failure.

The scheduler or resource manager should be able to replenish an exhausted context without forcing the server to abandon the request.

But a passive server must also support explicit cancellation and recovery.

The kernel alone cannot guarantee temporal isolation if the server holds shared locks while suspended.

---

# Discussion: 15. Can the proposed mechanisms actually implement the different policies?

This is the test I'd use to evaluate the design.

| Policy | Userspace data structures | Required kernel mechanisms |
|---|---|---|
| FIFO | Priority queues | Dispatch, preempt |
| Round-robin | FIFO queues, quantum | Dispatch, exhaustion event |
| MuQSS | Skip lists, virtual deadlines | Dispatch, retained slice accounting |
| EEVDF | Eligible tree, lag, virtual deadlines | Accurate execution accounting, dispatch |
| EDF | Deadline queue | Timers, preemption |
| Sporadic server | Replenishment queue | Accounting, timers, execution limits |
| SCHED_ISO-like | CPU usage history, fallback priority | Accounting, dispatch |
| Weighted fair | Weights, accumulated service | Accounting, dispatch |
| Hierarchical scheduler | Nested scheduling domains | Delegation, bounded execution |
| Client-funded RPC | Donation chain | Time handoff |
| Partitioned RT | Per-partition budgets | Domain-level execution limits |
| Global EDF | Global runnable queue | Multicore dispatch and migration |

The important question isn't whether a policy is easy to implement.

It's whether implementing it requires changing the kernel.

If you can implement all these policies using the same basic mechanisms, the abstraction boundary is probably in the right place.

Two reservations are worth noting. First, achieving high-performance implementations of all these policies may require optimized shared-memory dispatch and event interfaces. Second, implementing a policy is not the same as obtaining its theoretical real-time guarantees; those depend on the parent scheduler's service guarantees and system assumptions.

---

# Discussion: 16. One more radical alternative: remove `Time` from dispatch entirely

> **Not adopted** ([Decisions](#decisions), "Dispatch"): `Scheduler.Dispatch` names the Thread's bound Time (`x3`). As proposed here, a protected call keeps the current Time.

There's another design worth contemplating.

What if the kernel interface were simply:

```rust
Scheduler::dispatch(
    thread: ThreadCap,
    duration: Duration,
)
```

And `Time` were attached to the **current execution chain** rather than explicitly passed with every dispatch?

For example:

```text
Client A runs
    Time = A

A calls Server S
    Time remains A

S calls Driver D
    Time remains A
```

The kernel maintains a current execution context:

```rust
struct ExecutionContext {
    thread: ThreadId,
    time: TimeId,
    scheduler: SchedulerId,
}
```

A normal scheduler dispatch reuses the time authority associated with the selected thread or execution continuation.

An explicit operation is required only when changing the time authority:

```rust
Thread::bind_time(time)
```

Or:

```rust
Time::donate(thread)
```

This could make ordinary dispatch cheaper and simplify accounting across RPC.

However, it also risks coupling time authority to thread state in ways that become awkward for asymmetric scheduling or concurrent requests.

I'd therefore lean toward retaining explicit `Time` selection in the conceptual dispatch interface, while allowing the kernel to optimize common cases through cached bindings and IPC handoff.

---

# Discussion: 17. What I'd borrow from existing research

Three systems are particularly relevant.

**seL4 MCS** demonstrates that CPU execution budgets can be represented as separate capability-controlled objects and propagated across synchronous IPC. Its implementation keeps fixed-priority scheduling and sporadic replenishment in the kernel, so your design could go further by delegating those policies to userspace. [seL4 MCS](https://docs.sel4.systems/Tutorials/mcs.html)

**ghOSt** is probably the closest reference for actual userspace scheduling policy delegation. Its event interface, shared status information, generation-based validation, and transactional dispatch are directly relevant. Importantly, it demonstrates policies ranging from centralized FIFO to EDF without requiring each policy to be implemented as a new kernel scheduler. [ghOSt paper](https://research.google/pubs/ghost-fast-and-flexible-user-space-delegation-of-linux-scheduling/) [Google Research](https://research.google/pubs/ghost-fast-and-flexible-user-space-delegation-of-linux-scheduling/?utm_source=chatgpt.com)

**Scheduler Activations** (Anderson et al., 1992) is a foundational reference for kernel notifications of scheduling-relevant events and cooperation between kernel and userspace schedulers. It addresses several problems that reappear whenever scheduling decisions move out of the kernel. [Scheduler Activations](https://doi.org/10.1145/146941.146944) [DOI](https://doi.org/10.1145/146941.146944?utm_source=chatgpt.com)

I'd study ghOSt particularly closely before finalizing the dispatch ABI. Its handling of races between kernel events and userspace decisions is highly relevant to keeping the fast path efficient.

---

# Discussion: 18. My preferred architecture

> **A proposal, not the selected design.** What was adopted is the [Decisions](#decisions) section.

If I had to choose an initial design, it would look like this:

```text
                       ┌───────────────────┐
                       │ Root Scheduler    │
                       │                   │
                       │ Userspace policy  │
                       └─────────┬─────────┘
                                 │
                      delegates authority
                                 │
              ┌──────────────────┴──────────────────┐
              │                                     │
              ▼                                     ▼
    ┌───────────────────┐                ┌───────────────────┐
    │ Scheduler A       │                │ Scheduler B       │
    │                   │                │                   │
    │ EDF / Real-time   │                │ MuQSS / Fair      │
    └─────────┬─────────┘                └─────────┬─────────┘
              │                                     │
        dispatch()                              dispatch()
              │                                     │
       ┌──────┴──────┐                       ┌──────┴──────┐
       │             │                       │             │
       ▼             ▼                       ▼             ▼
    Thread A1     Thread A2               Thread B1     Thread B2
       │             │                       │             │
       ▼             ▼                       ▼             ▼
     Time A1       Time A2                 Time B1       Time B2
```

The kernel contains only:

```text
Thread state
Scheduling authority
Execution accounting
Execution limits
CPU dispatch
Scheduler activation
Event notification
Timer management
```

Everything else is userspace policy.

I'd introduce the mechanisms in three stages.

**Stage 1 — Basic userspace scheduling.** Implement `Thread`, `Scheduler`, and `Time`, plus dispatch, blocking/waking notifications, accurate CPU accounting, and execution timers. Use a single root scheduler initially. Test FIFO, RR, MuQSS-like, and EDF policies.

**Stage 2 — Temporal isolation and RPC.** Introduce separately enforceable execution allowances, authorized time transfer, and synchronous IPC donation. Test shared passive servers and exhaustion recovery.

**Stage 3 — Hierarchical and multicore scheduling.** Add delegated scheduler domains, parent-enforced execution limits, per-CPU scheduling agents, and batched remote dispatch.

This sequence gives you a useful kernel early without requiring you to solve hierarchical real-time scheduling before having a functional scheduler.

## One architectural decision I'd make now

I'd distinguish between these two concepts, even if they initially share one object:

- **`Time` is the accounting and execution-resource identity.**
- **`Scheduler` is the authority to decide how that resource gets spent.**

A scheduler can spend only time it is authorized to spend. A thread can execute using time it doesn't own. A time resource can move between threads without changing their scheduling authority.

That produces a nice symmetry with capability-based memory management: possession of a resource capability and authority to select how that resource is used are related but distinct.

I'd probably keep the name **`Time`**, rather than `Budget`, if the object supports accounting, transfer, and execution attribution. A budget is a quantity or limit; `Time` can represent a first-class computational resource. `Budget` might be the name of one field or operation on it.

**The hardest unresolved design decision is the relationship between a scheduler's own CPU execution and the CPU time it allocates to other threads.** I'd resolve that before settling the ABI. If scheduler execution and application execution share a parent-enforced allowance, hierarchical scheduling becomes relatively natural. If scheduler execution is privileged and unaccounted, you can still implement userspace policies, but strong temporal isolation becomes much harder to guarantee.

My central design preference is therefore:

> Keep policy decisions in userspace, make execution grants the kernel's fundamental scheduling primitive, and enforce CPU-time authority at scheduling-domain boundaries rather than embedding particular replenishment algorithms in the kernel.

---

**Why I'd choose authoritative shared state plus event notifications**:

**The kernel should guarantee that the scheduler can regain control, but it shouldn't necessarily switch to the scheduler thread on every event.**

This separates three concepts that are easy to conflate:

1. **State:** What is the current state of each thread?
2. **Notification:** What changed that might require a scheduling decision?
3. **Activation:** When does the userspace scheduler actually get CPU time?

C handles the first two. The third needs an explicit contract.

## 1. Why C is preferable

### A. Immediate scheduler activation

```text
Thread blocks
    → Kernel updates state
    → Kernel switches to scheduler
```

This is straightforward, particularly on a uniprocessor.

But it conflates an event with a scheduling decision.

For example, if a thread wakes on CPU 1 while CPU 0 is already executing a more important thread, there might be no reason to interrupt CPU 0 immediately.

It also creates unnecessary scheduler activations when several events occur in quick succession.

### B. Event queue only

```text
Thread blocks
    → Kernel queues event
    → Kernel wakes scheduler
```

The main problem is that an event queue is a history of transitions, not necessarily an authoritative representation of current state.

Consider:

```text
Thread A wakes
Thread A blocks
Thread A wakes again
```

The scheduler could encounter all three events after the thread is already runnable.

It then needs to reconstruct the current state from possibly outdated notifications.

Queue overflow, event coalescing, and concurrent state changes make this more difficult.

### C. Authoritative state + notifications

```text
Kernel:
    Updates ThreadStatus
    Increments generation
    Signals Scheduler

Userspace scheduler:
    Receives notification
    Reads ThreadStatus
    Makes scheduling decision
```

This is more robust.

The notification means **something changed**. The state record tells the scheduler **what is true now**.

The scheduler needn't reconstruct current thread state from the event history.

---

## 2. But notification and activation must be separate

This is where I'd modify the proposed C contract slightly.

Consider three events.

### Case 1: currently running thread blocks

```text
CPU 0:
    Thread A running
        ↓
    A blocks on IPC
        ↓
    Kernel records BLOCKED(A)
        ↓
    Kernel activates Scheduler S
        ↓
    S chooses Thread B
        ↓
    B executes
```

Here, the kernel **must find another execution context**, because A cannot continue.

If S is the designated scheduler for CPU 0, immediately activating it makes sense.

There is little benefit in waking S and then requiring some other scheduler to decide whether S should execute.

### Case 2: running thread exhausts its `Time`

```text
CPU 0:
    Thread A running
        ↓
    Time A exhausted
        ↓
    Kernel records exhaustion
        ↓
    Kernel activates Scheduler S
        ↓
    S selects another execution grant
```

Again, the CPU needs a scheduling decision.

I'd use an immediate scheduler activation, subject to the scheduler's own execution authority.

An important detail: **budget exhaustion should not change A's logical runnable state**. It ends the current execution grant.

For example:

```rust
ThreadStatus {
    state: Runnable,
    running: false,
    generation: 42,
}
```

The exhaustion notification belongs to the execution grant or `Time` object, not necessarily the thread itself.

### Case 3: blocked thread wakes

```text
CPU 0:
    Thread A running

CPU 1:
    Thread B wakes
        ↓
    Kernel records RUNNABLE(B)
        ↓
    Kernel signals Scheduler S
```

Here, immediate preemption isn't necessarily required.

S might decide that A should continue executing.

Or that B should execute on CPU 1.

Or that B should migrate to another CPU.

The kernel shouldn't impose any of those decisions.

Instead, waking B generates a notification, and the scheduler's activation configuration determines how promptly it receives CPU time.

This is the key distinction:

| Event | State update | Notification | Immediate activation |
|---|---|---|---|
| Running thread blocks | Yes | Yes | If a new dispatch is required |
| Running thread exhausts time | Yes | Yes | If a new dispatch is required |
| Blocked thread wakes | Yes | Yes | Policy/activation dependent |
| Running thread yields | Yes | Yes | Usually |
| Thread terminates | Yes | Yes | If it was running |
| Time is donated | Yes | Yes | Depends on affected execution |
| Scheduler timer expires | Timer event | Yes | According to timer semantics |

The kernel doesn't need to know the scheduler's priority policy to distinguish between *the current execution cannot continue* and *something else became runnable*.

---

## 3. The minimal shared state

I'd start with something like:

```rust
#[repr(C)]
struct ThreadStatus {
    sequence: AtomicU64,
    state: AtomicU32,
    cpu: AtomicI32,
}

#[repr(C)]
struct TimeStatus {
    sequence: AtomicU64,
    consumed: AtomicU64,
    remaining: AtomicU64,
}

#[repr(C)]
struct SchedulerStatus {
    sequence: AtomicU64,
    pending_events: AtomicU32,
}
```

These are conceptual structures, not yet a complete concurrent-memory ABI.

The kernel owns these records. Userspace maps them read-only.

The scheduler can inspect the latest state without a syscall.

For events, something like:

```rust
enum SchedulerEvent {
    ThreadRunnable(ThreadId),
    ThreadBlocked(ThreadId),
    ThreadExited(ThreadId),

    ExecutionStopped {
        thread: ThreadId,
        reason: StopReason,
    },

    TimeExhausted(TimeId),
    TimerExpired(TimerId),
}
```

Each event carries a relevant sequence number.

The kernel should increment the sequence number as part of publishing the state transition.

### Why sequence numbers matter

Suppose the scheduler observes:

```text
B is runnable, generation 10
```

Then B blocks before the scheduler dispatches it.

The kernel updates:

```text
B is blocked, generation 11
```

If the scheduler submits:

```rust
dispatch(
    thread = B,
    expected_generation = 10,
)
```

The kernel rejects the stale scheduling decision.

This is essentially the same problem addressed by ghOSt's agent and thread sequence numbers. ghOSt also exposes sequence numbers in shared-memory status words and validates scheduling transactions against concurrent state changes. [Computer Science](https://cs.stanford.edu/~jhumphri/documents/ghost.pdf?utm_source=chatgpt.com)

One nuance: simply having an atomic sequence counter doesn't make a multi-field status record an atomic snapshot. You need an explicit publication protocol, such as a sequence-lock-style reader loop with correctly defined memory ordering, or individually atomic fields with carefully specified consistency semantics.

---

## 4. Avoiding lost wakeups in `Scheduler.Wait`

This is a particularly important interface detail.

Consider:

```text
Scheduler:
    Read event queue → empty

Kernel:
    Thread B becomes runnable
    Signal scheduler

Scheduler:
    Call Scheduler.Wait()
```

If `Wait()` blindly blocks, the scheduler misses the notification and may sleep indefinitely.

I'd make `Scheduler.Wait` check a sequence number atomically with entering the wait state.

For example:

```rust
Scheduler::wait(expected_sequence: u64);
```

Semantics:

> Sleep only if the scheduler's event sequence still equals `expected_sequence`.

So:

```rust
loop {
    let sequence = scheduler.status.sequence.load(Acquire);

    process_events();
    schedule_runnable_threads();

    scheduler.wait(sequence);
}
```

If any relevant event arrived since `sequence` was read, `wait()` returns immediately.

If no event arrived, the kernel atomically registers the wait and blocks the scheduler.

This is effectively a futex-style compare-and-wait primitive.

It avoids lost wakeups without requiring the scheduler to hold a kernel lock.

For a real implementation, I'd also make event draining and dispatch commits sequence-aware so that the scheduler cannot accidentally sleep after overlooking work it still needs to process.

---

## 5. One thing I would *not* put in the event queue

I wouldn't require the event queue to be a complete history of every scheduling transition.

For example, if A goes through:

```text
Runnable → Blocked → Runnable → Blocked
```

while the scheduler isn't running, the kernel doesn't necessarily need to retain every transition.

If the scheduler only needs the current state, it can observe:

```text
A: Blocked
Generation: 17
```

The event notification can be coalesced.

This makes the interface substantially more scalable.

However, there is a distinction between **state-change notifications** and **events carrying information that cannot be reconstructed from current state**.

For example, a one-shot timer expiration or execution completion may carry information the scheduler needs even if the corresponding thread has subsequently changed state.

I'd use:

- Shared records for authoritative current state.
- Coalescible notifications for state changes.
- Reliable event records, or persistent completion records, for non-reconstructible events.

That avoids turning the kernel into a large event-history database while preserving correctness.

---

## 6. I'd make `Scheduler.Wait` both wait and dispatch

There's another optimization worth considering.

Instead of:

```rust
Scheduler::dispatch(thread, time, duration);
Scheduler::wait(sequence);
```

you could expose:

```rust
Scheduler::dispatch_and_wait(
    thread,
    time,
    duration,
    expected_sequence,
);
```

The kernel atomically:

1. Validates the dispatch decision.
2. Arms the execution allowance.
3. Installs the selected thread.
4. Suspends the scheduler agent.
5. Returns to the scheduler when a relevant event requires its attention.

This avoids a separate scheduling syscall and wait syscall.

It also makes the scheduler's handoff behavior explicit.

The normal userspace loop becomes:

```rust
loop {
    process_events();

    let next = policy.select_next();

    scheduler.dispatch_and_wait(
        next.thread,
        next.time,
        next.quantum,
        scheduler.sequence(),
    );
}
```

The kernel remains completely unaware of the scheduling policy.

One caveat: for centralized multicore schedulers, this synchronous interface isn't sufficient by itself. You'll still want a nonblocking dispatch or transaction mechanism that can schedule remote CPUs without putting the centralized agent to sleep after every decision.

---

## 7. My preferred final contract

I'd settle on **C with event-driven scheduler activation**, defined approximately as follows:

**Kernel responsibilities:**

1. Maintain authoritative scheduling-relevant state.
2. Publish state changes through shared records with sequence numbers.
3. Deliver notifications through `Scheduler.Wait`.
4. Ensure a scheduler is activated when its CPU requires another execution decision.
5. Permit other events to request scheduler activation without forcing immediate preemption.
6. Validate dispatch decisions against current kernel state.
7. Enforce `Time` exhaustion independently of scheduling policy.

**Userspace responsibilities:**

1. Maintain policy-specific runnable queues and accounting.
2. Interpret notifications and inspect current thread states.
3. Choose when and where threads execute.
4. Allocate or donate `Time`.
5. Decide whether waking a thread justifies preemption.
6. Implement fairness, priorities, deadlines, and replenishment.

### One last important refinement

I'd make **scheduler activation a property of the `Scheduler` capability/domain, not simply an ordinary wakeup of the scheduler thread**.

Otherwise, you reintroduce the bootstrap problem: who schedules the thread responsible for scheduling?

At the root, the kernel needs a narrowly defined activation mechanism. For child schedulers, activation must be constrained by the authority and execution capacity delegated by their parents.

That activation mechanism can coexist with `Scheduler.Wait`: the wait operation registers the scheduler agent's readiness to receive control, while the kernel's dispatcher determines when control is transferred according to the established scheduling-domain authority.

**So yes, this is the right foundation. But I'd describe the architecture as three independent mechanisms: authoritative state, event notification, and scheduler activation.**

That separation is what gives you the flexibility to implement everything from simple round-robin through MuQSS to hierarchical EDF without turning notifications themselves into an implicit kernel scheduling policy.

---

## 8. The root dispatcher's first priority policy (2026-10-29)

The first policy that needed more than round-robin shows the split above at work. The root dispatcher (`userspace/dispatcher`) dispatched round-robin with a 10 ms slice, and a woken or preempted Thread went to the back of the round. With three CPU-bound peers (Wood's engine, the compositor, the lava lamp), Wood's audio playback Thread, which works about 1 ms per 11.6 ms period, waited 20–30 ms after each preemption ([audio-profiling.md](audio-profiling.md)).

The fix is entirely userspace: fixed priority in the root dispatcher, declared with each Thread and carried by Kickstart, round-robin within a level. The kernel already reported the wakeup as an activation (`THREAD_RUNNABLE`); only the decision changed. The level is a byte so more fit later. The first round deliberately had no starvation guard, on the expectation that every Thread above normal blocks most of the time; the board disproved it the same day (below). The selected rule is in the [scheduling design](scheduling-design.md#decisions) (root dispatcher policy); the work items are in [`scheduling-completed.md`](scheduling-completed.md).

The second level came the same day, from the board: keys felt sluggish, and some presses and releases were lost, while Wood played. The maintainer: "the usb is probably in need of priority between the main app and the audio?". A key crosses several Threads (the DWC2 component's interrupt and poller Threads, the HID component, the compositor's input Thread, then Wood's engine), and at normal level each hop waited behind the CPU-bound peers. That costs more than latency: a split interrupt poll of a low-speed keyboard behind the LAN9514's transaction translator wakes the interrupt Thread and then the poller between its start split and its complete split, and the hub keeps a periodic result only for the microframes right after the start split (USB 2.0 §11.18.4, §11.18.6), so a late complete split loses the report the device already had acknowledged. A level between normal and high (input) puts those cheap, blocking Threads ahead of the programs and keeps audio ahead of them. The input Threads must stay cheap: the compositor's composing stays on its normal frame Thread, so it does not outrank Wood's engine.

The guard came with the next board run. `boot-wood-test` froze about 2 s after boot: the log stopped and the audio DMA ring replayed its last buffers. The DWC2 component's Threads, now input level, still paused by spinning on the counter (a 1 ms pause between NAK retries for up to a second, port waits of up to 3 s), and a spinning input Thread keeps every normal Thread (the log server, the audio driver, `usb-core`, Wood's engine) off the CPU; Wood's high playback Thread then waits on a starved audio driver. The maintainer: "delay_ms(1) should just park the thread until 1ms passes, not busy wait / add starvation guard too". Both halves are userspace. The driver parks on a timed wait instead of spinning. The dispatcher already learns everything the guard needs from the record it reads anyway: `run_time_ns` says how long a Thread ran, and `last_stop` says whether it blocked or was preempted. A raised Thread that has run a whole slice since it last blocked is dispatched at normal level until it blocks. No kernel information was added, and a kernel that charged and reported time without knowing about priorities was enough for both the policy and its guard.
