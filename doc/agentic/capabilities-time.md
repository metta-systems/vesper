# Time and userspace scheduling: gaps

Status as of 2026-10-16. Sources: `kernel/nucleus/src/{timer.rs,objects/time.rs,objects/scheduler.rs,objects/sched.rs,objects/thread.rs,api/event_count.rs}`, `libs/object/src/thread.rs`, `doc/agentic/capabilities-implementation-plan.md` (Phases 4, 6, 7), `doc/agentic/capabilities-decisions.md` (D5, D8).

## Time: missing

- Nothing charges any Time. Budgets never drain or expire (`objects/time.rs`).
- `timer::arm_at(deadline_ns)` is a one-shot primitive, and the tick is built on it (`timer.rs`). The tick itself is still a fixed 10 ms periodic timer. It must be replaced by per-dispatch deadlines.
- No deadline, sleep or alarm. `EventCount.Await` rejects finite timeouts (`api/event_count.rs:108`).
- No clock-read capability for EL0.
- PPC per-call time is recorded but not charged on Return.
- Open D8: root budget finite or infinite, budget on Time delete, dispatch register schema, scheduling-token race protection, clock source and rounding.

## Userspace scheduling: missing

- No dispatch syscall (Thread, Time, priority, timeout) and no Yield.
- No scheduler activations. Nothing is posted to the scheduler Thread on block, wake, expiry or preemption. The kernel run queue is FIFO and not priority-aware.
- `Scheduler.ShareRegion` returns `InvalidOperation`. The shared record table, publication protocol and record schema are undecided (D5).
- No sub-schedulers, hierarchical derivation or Thread donation.
- Thread `Suspend`, `Resume` and `Grant` return `InvalidOperation`. `ThreadKey::state`, `time_used_ns` and `pending_notifications` are `unimplemented!()` (`libs/object/src/thread.rs:96-109`).
- Wake-time preemption by priority-chain comparison is unbuilt. So is the IRQ receiver Thread with its own Time.
- Phase 6 race and multi-waiter tests for Notification and EventCount are unchecked.
- Single core only. Multicore Time is classed "very large".

## Suggested order

1. Settle D8 and D5 (record schema, dispatch op). These are blockers.
2. Charging, plus the one-shot timer.
3. Dispatch op with expiry and block events posted to the scheduler Thread, then ShareRegion records.
4. Timeouts on Await, then sleep.
5. Thread Suspend/Resume, then sub-schedulers and donation.

---

# Research: Time vs Composite tcap

Composite's tcaps are closer to vesper's Time than I expected. The main differences are what a budget is attached to, and that Composite puts priority inside the budget. Sources: vesper's doc/object_types/core/time.md, doc/capabilities-contract.md ("Time and userspace scheduling"), D8 and the objects/time.rs sketch, compared with Composite main's kernel/tcap.c, include/tcap.h and the tcap operations in capinv.c.

Where they agree

┌────────────────────────┬─────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│      Vesper Time       │                                               Composite tcap                                                │
├────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ "Storage cannot mint   │ TCAP_ACTIVATE makes a tcap out of kernel memory with 0 cycles. Budget only arrives by transfer from another │
│ budget"                │  tcap.                                                                                                      │
├────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ Split                  │ No split operation: make an empty tcap, then TCAP_TRANSFER cycles into it. The source is reduced and the    │
│                        │ destination increased in one step.                                                                          │
├────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ Merge                  │ TCAP_MERGE: move everything into the destination, then delete the source. It refuses if the source is still │
│                        │  referenced (refcnt > 1).                                                                                   │
├────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ Query                  │ tcap_introspect(TCAP_GET_BUDGET)                                                                            │
├────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ Conservation           │ Strict, except for a special infinite budget value: a source with infinite budget can hand out infinite     │
│                        │ budget. The destination saturates rather than overflowing.                                                  │
├────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ Expiry gives an        │ When a tcap runs out, the kernel switches to that tcap's scheduler thread and notifies it (expended_process │
│ explicit continuation  │  → notify_parent). If that scheduler is also out, it takes the next tcap from a per-core active list.       │
└────────────────────────┴─────────────────────────────────────────────────────────────────────────────────────────────────────────────┘

Where they differ

1. A budget is not owned by a thread; the scheduler picks one per dispatch.
   - The user-level switch call takes (thread, tcap, priority, timeout). The CPU time used is charged to whatever tcap is current, not to the thread.
   - Only threads that own a receive endpoint have their own tcap. That tcap is used when the kernel wakes the thread because of an event, such as an interrupt or an asynchronous send.
   - Vesper's Donate ("authorize execution of a target using a budget, suspend the donor") merges budget transfer and dispatch into one operation. Composite keeps them separate: TRANSFER moves budget, and a switch runs a thread on a chosen tcap.
   - TCAP_DELEGATE combines the two: it sends budget to the receiving thread and may switch to it straight away.
2. Priority lives in the tcap, as a delegation chain. Vesper's Time has no priority at all.
   - Each tcap carries up to 16 (scheduler id, priority) pairs, one per scheduler it has passed through.
   - tcap_higher_prio compares two tcaps only at schedulers they share. This partial order is how the kernel decides, on an interrupt, whether to preempt without asking any user-level scheduler.
   - Vesper puts the scheduler hierarchy in Scheduler capabilities and shared records. It has no kernel-side rule yet for "this interrupt is more urgent than the running thread". That gap is exactly what tcaps fill, and it links back to the Brand/IRQHandler question.
3. Transfer is permanent; nothing is lent.
   - There is no parent link and no automatic return of unused budget. The only way back is an explicit Merge.
   - The "revoke by bumping an epoch" scheme described in the header comments is not implemented.
   - When a tcap is used up, it drops all but its current scheduler's priority from the chain (the code calls this "declassifying" the time).
   - So for D8, Composite's answer is: transfer, not loan.
   - Vesper's sketch has a parent field meant for returning time to the parent, which is the opposite choice.
4. No deadline in the budget; timeouts are given per switch.
   - Vesper's Time sketch has a deadline.
   - Composite programs a one-shot timer at each switch, for whichever comes first: the budget running out or the timeout given with that switch (tcap_timer_update).
   - There is no periodic tick, and the timer is turned off entirely when the budget is infinite and no timeout is set. Vesper today has a fixed 10 ms periodic tick.
5. Budgets are per core.
   - Every tcap belongs to one core, and transfer, delegate and switch reject a tcap from another core.
   - That is Composite's answer to D8's "multicore budget ownership" question.
6. Units. Composite counts raw cycle-counter ticks (TSC). Vesper's sketch says microseconds and the contract suggests nanoseconds; D8 is still open.

How far apart they are
The accounting model (empty creation, transfer, merge, query, conservation, an infinite root budget) already matches vesper's contract. The real distance is in three design choices:
- Priority: whether Time carries scheduling priority so the kernel can make the interrupt-preemption call itself, Composite style, or whether that stays in Scheduler records.
- Lifetime: whether time is lent (vesper's parent and return) or transferred (Composite).
- Binding: whether a budget is tied to a Thread or picked per dispatch. Composite picks per dispatch and gives event-receiving threads their own budget.

If you adopt the Composite answers, D8 is largely settled. The Brand question then goes away too: what a brand gave an event was its own budget and priority, and that becomes a Time bound to the Notification's receiver, which is the role seL4's MCS configuration gives a SchedContext bound to a Notification.

## Per-dispatch vs per-thread time budget

I've recorded your four decisions in the plan file. Here is the comparison you asked for, using vesper's current mechanisms: migrating-thread protected procedure calls (PPC, where the calling Thread itself moves into the target component), the kernel's first-in-first-out run queue, and completion wakeups.

The two models
- Per-thread (seL4 MCS: a SchedContext bound to a thread; NOVA: a scheduling context per execution context): each Thread holds a Time, and whatever runs that Thread spends that Time.
- Per-dispatch (Composite): Threads hold no budget. The scheduler names (Thread, Time, priority, timeout) on every switch, and the kernel charges the "current Time".

What changes functionally

┌──────────────────────────────┬─────────────────────────────────────┬─────────────────────────────────────────────────────────────────┐
│           Concern            │             Per-thread              │                          Per-dispatch                           │
├──────────────────────────────┼─────────────────────────────────────┼─────────────────────────────────────────────────────────────────┤
│ Kernel wakes a thread        │ The woken thread's Time gives       │ The kernel has nothing to compare unless the receiver has its   │
│ (Notification, IRQ,          │ priority and budget, so the kernel  │ own Time. Composite therefore gives every receive-endpoint      │
│ completion)                  │ can decide on preemption itself.    │ thread a tcap, which is per-thread binding again, just for      │
│                              │                                     │ those threads.                                                  │
├──────────────────────────────┼─────────────────────────────────────┼─────────────────────────────────────────────────────────────────┤
│                              │                                     │ No Time to use. Either control goes back to the scheduler       │
│ Thread blocks and the kernel │ Works as today: the next thread     │ thread on every block (Composite does this), or the kernel      │
│  picks the next one          │ runs on its own Time.               │ needs a default Time. Vesper's in-kernel FIFO and               │
│                              │                                     │ completion-wake paths would have to change.                     │
├──────────────────────────────┼─────────────────────────────────────┼─────────────────────────────────────────────────────────────────┤
│ Protected procedure call     │ The Time travels with the migrating │ The current Time doesn't change on Call, so the client pays     │
│ (PPC)                        │  Thread, so the server's work is    │ here too. No difference for vesper.                             │
│                              │ charged to the client.              │                                                                 │
├──────────────────────────────┼─────────────────────────────────────┼─────────────────────────────────────────────────────────────────┤
│ Budget shared by a group (a  │ Not possible without letting        │ Natural: a few Times act as scheduler-level accounts, and any   │
│ component, a server pool, a  │ several Threads share one Time,     │ thread can run on them.                                         │
│ criticality class)           │ which brings sharing rules with it. │                                                                 │
├──────────────────────────────┼─────────────────────────────────────┼─────────────────────────────────────────────────────────────────┤
│                              │ Blocked threads keep their budget.  │                                                                 │
│ Budget left in idle threads  │ With transfer-only, the scheduler   │ Budget stays with the scheduler until it dispatches.            │
│                              │ has to Merge it back explicitly.    │                                                                 │
├──────────────────────────────┼─────────────────────────────────────┼─────────────────────────────────────────────────────────────────┤
│ Number of Time objects       │ One per schedulable Thread          │ One per account, plus one per receiver thread                   │
├──────────────────────────────┼─────────────────────────────────────┼─────────────────────────────────────────────────────────────────┤
│                              │                                     │ Resume plus a Time key, priority and timeout. It also needs     │
│ Cost of a dispatch           │ Resume the Thread                   │ Composite's scheduling token to catch races between user-level  │
│                              │                                     │ dispatch and kernel events.                                     │
├──────────────────────────────┼─────────────────────────────────────┼─────────────────────────────────────────────────────────────────┤
│ Per-thread accounting        │ Comes from the Time                 │ Must be kept separately; Composite keeps thd->exec              │
│ (scheduler records)          │                                     │                                                                 │
├──────────────────────────────┼─────────────────────────────────────┼─────────────────────────────────────────────────────────────────┤
│                              │ Must decide what happens to its     │                                                                 │
│ Retiring a Thread            │ bound Time; transfer-only means it  │ Nothing to do                                                   │
│                              │ is either merged out first or lost. │                                                                 │
├──────────────────────────────┼─────────────────────────────────────┼─────────────────────────────────────────────────────────────────┤
│ Your IRQ model               │ Fits directly: the receiver's Time  │ Fits too, but only because receivers are the per-thread         │
│                              │ sets its priority.                  │ exception.                                                      │
└──────────────────────────────┴─────────────────────────────────────┴─────────────────────────────────────────────────────────────────┘

Conclusion. Your IRQ model already needs per-thread binding for receivers, so per-dispatch can only ever be a hybrid. The hybrid is a strict superset of per-thread:
- "dispatch Thread T" means "dispatch T on T's own Time";
- an explicit dispatch-with-Time operation can be added later without breaking the ABI.

What per-dispatch really adds is group budgets and hierarchical accounts kept by the scheduler, at the cost of reworking the kernel's run queue and block paths.

---
