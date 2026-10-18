# Scheduler

| | |
|---|---|
| Wire type | `0x05` (core) |
| Backing | Kernel object: the root is carved from the boot Untyped by Kickstart, any other from an Untyped by `Untyped.Retype` |
| Operations | ShareRegion, ConfigureThread, StartThread, AssignRecord, BindEventCount, BindDispatcher, Dispatch, ConfigureActivation, ArmTimer, Adopt, MoveThread |

## Purpose

A Scheduler capability is the authority to make Threads run. A Thread created by [`Untyped.Retype`](untyped.md#retype) is inert: it has no AddressSpace and no execution state, and it never runs. A Scheduler turns it into a Thread that runs, by configuring it and then starting it. Schedulers form a tree: the root, and children that a parent adopts. Every operation works on any Scheduler for a holder with `Write`, but a child has no execution authority until its parent has adopted it and dispatched its dispatcher (see [Child Schedulers](#child-schedulers)). The first component receives the root Scheduler at `KeySlot::ROOT_SCHEDULER` (18).

A Scheduler also owns a table of scheduling records. The nucleus writes each of its Threads' state into a record the scheduler can read without a system call, and advances an EventCount when a record changes.

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | ShareRegion | `x2` Frame, `x3` first slot, `x4` stride in bytes | `Write` on the Scheduler; `Read` and `Write` on the Frame | — |
| 1 | ConfigureThread | `x2` Thread, `x3` AddressSpace, `x4` entry, `x5` stack pointer, `x6` Time, `x7` argument | `Write` on the Scheduler, Thread and Time; `Map` on the AddressSpace | — |
| 2 | StartThread | `x2` Thread | `Write` on the Scheduler and Thread | — |
| 3 | AssignRecord | `x2` Thread, `x3` slot | `Write` on the Scheduler and Thread | — |
| 4 | BindEventCount | `x2` EventCount | `Write` on the Scheduler; `Send` on the EventCount | — |
| 5 | BindDispatcher | `x2` Thread | `Write` on the Scheduler; `Write` on the Thread | — |
| 6 | Dispatch | `x2` Thread or 0, `x3` Time or 0, `x4` limit ns, `x5` generation, `x6`, `x7` zero | `Write` on the Scheduler | `x1` reasons, `x2` events |
| 7 | ConfigureActivation | `x2` mask | `Write` on the Scheduler | — |
| 8 | ArmTimer | `x2` deadline in nanoseconds (0 disarms) | `Write` on the Scheduler | `x1` the time now, `x2` the previous deadline |
| 9 | Adopt | `x2` child Scheduler | `Write` on the Scheduler and the child | — |
| 10 | MoveThread | `x2` Thread, `x3` destination Scheduler, `x4` slot | `Write` on the Scheduler, the Thread and the destination | — |

`Read` is rights bit 0, `Write` and `Map` are bit 1. `SchedulerKey` in `libs/object` wraps the operations. Unused argument registers must be zero.

### ShareRegion

Adds the Frame `x2` to the scheduler's record table as a region covering slots from `x3`, each `x4` bytes apart. The Frame must be ordinary RAM and already mapped; the AddressSpace it is mapped in is where the scheduler reads the records.

- The first region starts at slot 0 and fixes the AddressSpace and the stride. Later regions must be mapped in the same AddressSpace and use the same stride.
- The stride is a multiple of 64, at least 128 and at most the Frame size. A Frame covers `size / stride` slots.
- Regions never overlap in slots or in memory, so sharing a Frame twice fails. A table has at most 8 regions.
- Slot 0 holds the scheduler record, initialized when the first region is shared. The first 128 bytes of a Thread slot are the nucleus's; the rest of a slot belongs to the scheduler.

| Error | Cause |
|---|---|
| `InvalidOperation` | A device Frame; a first region that does not start at slot 0; an invalid stride; a different AddressSpace or stride from the first region; overlapping slots or memory; a ninth region; a slot or stride wider than 32 bits |
| `NotMapped` | The Frame is not mapped |
| `InsufficientRights` | A missing right |
| `TypeMismatch` | `x2` is not a Frame |

### ConfigureThread

Binds the inert Thread `x2` to the AddressSpace `x3`, an EL0 start state (entry `x4`, stack pointer `x5`, `x0` set to `x7` on first entry) and its own Time `x6`. The Thread records the Time it runs on and the Scheduler that configured it, and the Time becomes the Thread's own: while the Thread is live no budget can be taken out of it and no other Thread can be bound to it (see [Time](time.md#implementation)). The caller keeps the Time capability.

| Error | Cause |
|---|---|
| `InvalidOperation` | The Thread is not inert (already configured or started); the entry is not 4-byte aligned or not below the end of the user address range; the stack pointer is not 16-byte aligned or above that end; the Time belongs to another core or is already bound to a live Thread |
| `NotBound` | The AddressSpace has no KeyTable bound ([`AddressSpace.Bind`](../arch/address_space.md#bind)) |
| `NotMapped` | The AddressSpace has no translation root |
| `NoAsidAssigned` | The AddressSpace has no ASID |
| `InsufficientRights` | A missing right |
| `TypeMismatch` | A key names the wrong kind |

### StartThread

Makes the configured Thread `x2` runnable; it first runs its configured start state. A Thread whose Time holds no budget is runnable but waits off the run queue until budget arrives. A Thread that is inert or already started fails with `InvalidOperation`.

### AssignRecord

Links the Thread `x2` to record slot `x3` and publishes its record at once. The slot must be at least 1 and lie in a shared region, and the Scheduler must be the one that configured the Thread. The nucleus does not check that the slot is unused: the table is the scheduler's own memory, and the record's `incarnation` shows the scheduler that a slot now describes a different Thread. A Thread with no assigned slot has no record.

| Error | Cause |
|---|---|
| `InvalidOperation` | No table yet; slot 0 or a slot outside every region; a Thread this Scheduler did not configure |
| `InsufficientRights` | A missing right |
| `TypeMismatch` | A key names the wrong kind |

### BindEventCount

Binds the EventCount `x2`. After the nucleus publishes a change to the state, flags, block reason or stop reason of any record in the table, it advances the EventCount by one, so a scheduler that waits on it with [`EventCount.Await`](event_count.md#await) learns of the change. A refresh of only the counters does not advance it. Binding is once-only; a second bind fails with `InvalidOperation`. A retired EventCount ends the binding.

### BindDispatcher

For a child Scheduler the dispatcher is a Thread of its parent: it must have been configured by the parent, is not started by the bind (the parent starts it, and dispatches it), and the child must have been adopted. The bind marks the Thread as the dispatcher of the child: its record carries the `DISPATCHER` flag. After the dispatcher of a child faults or is retired, another can be bound (see [Child Schedulers](#child-schedulers)).

Binds the Thread `x2` as the scheduler's *dispatcher*, the Thread that decides what runs, and starts it. The Thread must have been configured by this Scheduler, and binding is once-only. From then on every other Thread this Scheduler configured, and every one it starts later, is dispatcher-managed: it is published in its record and runs only when the dispatcher names it with `Dispatch`. A Scheduler with no bound dispatcher runs nothing: its Threads wait. Kickstart binds the root dispatcher (`userspace/dispatcher`, a round-robin policy) in the production image, after it has started every driver and program, and registers the boot Thread with it. The test kernels run on a dispatcher too: the ones that load drivers bundle the same component, the others use the fixture dispatcher of `libkicktest`.

| Error | Cause |
|---|---|
| `InvalidOperation` | A dispatcher is already bound; the Thread was not configured by this Scheduler; a nonzero unused register |
| `TypeMismatch` | `x2` is not a Thread |

### Dispatch

Blocks the dispatcher until something needs its attention, and meanwhile runs one Thread. Only the bound dispatcher may call it.

| Register | Meaning |
|---|---|
| `x2` | Thread key to run, or 0 to wait with nothing dispatched |
| `x3` | the Thread's own Time key, or 0 |
| `x4` | limit in nanoseconds (must be nonzero; `u64::MAX` for none) |
| `x5` | the generation the dispatcher last saw in the Thread's record |
| `x6`, `x7` | must be 0 (`x6` is the CPU, only CPU 0 exists) |

The call returns, when the dispatcher is activated, `x1` the reasons as bits (EVENT 1, LIMIT 2, EXHAUSTED 4, BLOCKED 8, YIELDED 16, FAULTED 32, EXITED 64) and `x2` the scheduler's event count at that moment. If an activation was raised while the dispatcher ran, the call returns it at once and runs nothing.

The named Thread must belong to this Scheduler, not be the dispatcher, be runnable with budget and off every queue, and its generation must equal `x5`. The Time operand, if given, must be that Thread's own bound Time. The Thread runs until it blocks, faults, exits, yields, exhausts its Time, reaches the limit, or an unmasked event activates the dispatcher. Time is charged to the Thread's own Time as it runs.

| Error | Cause |
|---|---|
| `StaleGeneration` (42) | The Thread's generation differs from `x5`; the current generation is returned so the dispatcher can retry |
| `InvalidOperation` | The caller is not the bound dispatcher; a Thread of another Scheduler or the dispatcher itself; a Thread that cannot run now; a zero limit; a nonzero CPU or reserved register; a Time that is not the Thread's own |
| `TypeMismatch`, `InsufficientRights` | A key names the wrong kind or lacks `Write` (Time) |

### ArmTimer

Arms the dispatcher's timer for the absolute deadline `x2`, in nanoseconds on the monotonic clock (the physical counter, nanoseconds since it started; 0 disarms). Only the bound dispatcher may call it, and it has one timer: arming replaces the previous deadline. The call returns `x1` the time now and `x2` the previous deadline (0 if none). When the clock reaches the deadline the timer disarms itself and activates the dispatcher with the `TIMER` reason (`1 << 7`), whatever the activation mask says; a deadline already in the past fires at the next kernel exit. A timer ends with its dispatcher. The deadline is converted to counter ticks rounded up and kept in the nucleus's small table of dispatcher timers, which, with the wait timeouts, decides when the one-shot hardware timer next fires.

EL0 reads the same clock with no system call: `CNTVCT_EL0` and `CNTFRQ_EL0` (the nucleus leaves the virtual counter enabled for EL0 with a zero offset); `libuser::clock::now_ns` does the conversion.

| Error | Cause |
|---|---|
| `InvalidOperation` | The caller is not the bound dispatcher, or a nonzero unused register |
| `PoolExhausted` | The table of dispatcher timers is full (8 Schedulers) |
| `InsufficientRights` | Missing `Write` |

### Adopt

Makes the Scheduler `x2` a child of the invoked one. The parent is set once and never changes. The child must be fresh: it has no parent, is not the root, and has no children, Threads or dispatcher. A Scheduler with nothing in it has no descendants, so adopting it cannot close a cycle. The tree is at most four levels deep: the root and three below it.

| Error | Cause |
|---|---|
| `InvalidOperation` | The child is the invoked Scheduler, the root, already adopted, or not fresh; the invoked Scheduler is at the depth limit; a nonzero unused register |
| `InsufficientRights` | A missing right |
| `TypeMismatch` | A key names the wrong kind |

### MoveThread

Moves the Thread `x2` to the Scheduler `x3`, linking it to record slot `x4` of that Scheduler's table. Invoked on the Thread's owner, the destination is a direct child of the invoked Scheduler (donation); invoked on the owner's parent, the destination is the invoked Scheduler itself (taking the Thread back). The Thread must be started and not running, dispatched or a dispatcher; a blocked or suspended Thread moves with its wait, completion and Time untouched. The record it leaves reads `Donated`, which the old owner may reuse. A Thread that can run is announced to the new owner as for any wakeup; the first-in, first-out queue drops it.

| Error | Cause |
|---|---|
| `InvalidOperation` | The move is not between a Scheduler and its direct child; the Thread is inert, running, dispatched or a dispatcher; the slot is 0 or not covered by the destination's table; a nonzero unused register |
| `InsufficientRights` | A missing right |
| `TypeMismatch` | A key names the wrong kind |

### ConfigureActivation

Sets which events activate a parked dispatcher besides the ones that always do. The mask has bit 0 `THREAD_RUNNABLE` (a Thread became runnable, including a blocked one waking) and bit 1 `TIME_AVAILABLE` (budget arrived for a Thread that was waiting for it). The dispatched Thread's blocking, fault, exit, yield, exhaustion and limit always activate it. The initial mask is empty. Bits outside the mask fail with `InvalidOperation`. A masked event raised while the dispatcher is not parked is kept as a pending bit and returned by the next `Dispatch`. When the dispatcher activates it is made the next to run and the timer fires at once, so it runs before the preempted Thread.

## Child Schedulers

A child Scheduler runs inside a **grant** from its parent.

- **Dispatcher.** The child's dispatcher is a Thread of the parent, on a Time the parent Transferred in. The parent dispatches it like any other Thread of its own, with a limit; that limit is the grant.
- **Inside the grant.** While the parent's dispatch of the child dispatcher is in force, the child's own `Dispatch` calls run its Threads with limits clamped to what is left of the grant, so child Threads run without a parent activation. The end of the grant shows in the child's Scheduler record as `grant_end_ns` (0 when none is in force).
- **The grant ends** when its limit passes, when the parent preempts, or when the child dispatcher yields the CPU without handing it to a Thread (it waits with nothing dispatched, faults or exits). The running child Thread is stopped and stays runnable; the parent's dispatcher is activated with `LIMIT` (`BLOCKED`, `FAULTED` or `EXITED` for the child dispatcher itself); the child dispatcher stays parked in `Dispatch`, which completes with `LIMIT` when the parent next dispatches it.
- **Activation.** A child activation while the grant is in force runs the child dispatcher at once. Without a grant the parked `Dispatch` completes and the child dispatcher becomes an ordinary runnable Thread of the parent: its record is published, and the parent's dispatcher is activated only if the parent's mask has `THREAD_RUNNABLE`. A child never forces its parent.
- **Exhaustion** of the child dispatcher is ordinary exhaustion of a Thread of the parent; its record has the `DISPATCHER` flag so that the parent can tell.
- **Failure.** When the child dispatcher faults or is retired, the child has no dispatcher: its Threads that can be are suspended, its grant and timer end, and the parent's dispatcher is activated with `SCHED_DOMAIN_FAILED` (`1 << 8`, mandatory). The parent can retire the child, bind another dispatcher, or resume the Threads.
- **No dispatcher yet.** A Thread of a Scheduler with no dispatcher, root or not, does not run.

## Records

The records are in [`libobject::scheduler_record`](../../../libs/object/src/scheduler_record.rs). Each is 128 bytes and 64-byte aligned, with fixed-width little-endian fields, all accessed atomically. The scheduler can write the pages it maps, but the nucleus never reads a record back, so a scheduler that overwrites one only misleads itself.

**Scheduler record (slot 0)**

| Offset | Field |
|---:|---|
x (u32, 1), `record_size` (u32, 128) |
| 16 | `events` (u64): events published, equal to the bound EventCount |
| 24 | `timer_expirations` (u64) |
| 32 | `dispatcher_state` (u32), `flags` (u32) |
| 40 | `interrupt_ns` (u64): the total time spent handling interrupts, rounded up; refreshed with every publication and at each interrupt entry that interrupts one of the Scheduler's Threads, and not itself an event |
| 48 | `grant_end_ns` (u64): when the parent's grant to this Scheduler ends, 0 for none |
| 56 | `idle_ns` (u64): the total time the nucleus had nothing to run and waited for an interrupt, rounded up, excluding the interrupts handled meanwhile; published when the wait ends, and not itself an event |

**Thread record (slots 1 and up)**

| Offset | Field |
|---:|---|
| 0 | `sequence` (u64) |
| 8 | `generation` (u64): the Thread's state-change count |
| 16 | `incarnation` (u32), `state` (u32, a `ThreadState`) |
| 24 | `flags` (u32), `block_reason` (u32) |
| 32 | `last_stop` (u32), `cpu` (u32) |
| 40 | `run_time_ns` (u64), rounded up |
| 48 | `last_run_ns` (u64), rounded up |
| 56 | `budget_remaining_ns` (u64), rounded down |

- **Flags:** bit 0 `AWAITING_BUDGET` (runnable, but its Time is exhausted), bit 1 `HAS_TIME`, bit 2 `DISPATCHER` (the Thread is a Scheduler's dispatcher; for a Thread of a parent it tells the parent that the Thread runs a domain). Without a bound Time `budget_remaining_ns` is `u64::MAX`.
- **State:** `Inactive` for a Thread that is not started, `Runnable` for a queued Thread, one waiting for budget and a woken waiter that has not resumed yet, `Running`, `Blocked`, `Faulted`, and `Dying` once the Thread is retired.
- **Block reason:** none, Notification wait, EventCount await, protected procedure call.
- **Stop reason:** blocked, slice end, budget exhausted, yielded, replaced, exited, faulted.
- **`cpu`:** the CPU the Thread runs on, or `u32::MAX`.
- **Freshness:** the counters are as of the last published change, not of the moment you read them; `Time.Query` gives the exact budget.

The nucleus is the only writer. It makes `sequence` odd, writes the fields, then makes it even with release ordering. A reader loads `sequence` with acquire ordering, retries while it is odd, reads the fields, and retries if `sequence` changed; `ThreadRecord::try_snapshot` and `SchedulerRecord::try_snapshot` do this. `sequence` only detects torn reads; `generation` counts the Thread's state changes.

## Implementation

The root Scheduler is carved by Kickstart; `Untyped.Retype` makes inert ones (no parent, no Threads, no dispatcher), which have no execution authority until a parent adopts them. No queue runs Threads: a Thread runs only when its Scheduler's dispatcher dispatches it, and a Scheduler with no dispatcher runs nothing. The only list the nucleus keeps is the dispatchers an activation made ready to run next. The nucleus tracks which Thread the dispatcher dispatched and its limit, charges its Time as it runs, and at every kernel exit programs the one-shot timer for the nearest of the limit, the Time's exhaustion, and an immediate fire when an activation is waiting for the dispatcher.

The nucleus publishes a Thread's record when it starts, queues, moves to another Scheduler (`Donated` in the record it leaves), waits for budget, runs, blocks, stops, faults or is retired, and when `AssignRecord` links it. Retiring a Thread publishes `Dying` with its incarnation and drops the slot, and the scheduler may reuse the slot after it sees that. A shared Frame must stay mapped and must not be retired while the table uses it; the nucleus does not reclaim Frame memory.

## Planned changes

Non-blocking dispatch and replacing the running Thread on other CPUs (future kernel multicore work).
