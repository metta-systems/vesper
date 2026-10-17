# Scheduler

| | |
|---|---|
| Wire type | `0x05` (core) |
| Backing | Kernel object carved from the boot Untyped by Kickstart |
| Operations | ShareRegion, ConfigureThread, StartThread, AssignRecord, BindEventCount (on the root Scheduler) |

## Purpose

A Scheduler capability is the authority to make Threads run. A Thread created by [`Untyped.Retype`](untyped.md#retype) is inert: it has no AddressSpace and no execution state, and it never runs. Only the root Scheduler turns it into a Thread that runs, by configuring it and then starting it. The first component receives the root Scheduler at `KeySlot::ROOT_SCHEDULER` (18).

A Scheduler also owns a table of scheduling records. The nucleus writes each of its Threads' state into a record the scheduler can read without a system call, and advances an EventCount when a record changes.

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | ShareRegion | `x2` Frame, `x3` first slot, `x4` stride in bytes | `Write` on the Scheduler; `Read` and `Write` on the Frame | — |
| 1 | ConfigureThread | `x2` Thread, `x3` AddressSpace, `x4` entry, `x5` stack pointer, `x6` Time, `x7` argument | `Write` on the Scheduler, Thread and Time; `Map` on the AddressSpace | — |
| 2 | StartThread | `x2` Thread | `Write` on the Scheduler and Thread | — |
| 3 | AssignRecord | `x2` Thread, `x3` slot | `Write` on the Scheduler and Thread | — |
| 4 | BindEventCount | `x2` EventCount | `Write` on the Scheduler; `Send` on the EventCount | — |

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
| `InsufficientRights` | A missing right, or the Scheduler is not the root |
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

## Records

The records are in [`libobject::scheduler_record`](../../../libs/object/src/scheduler_record.rs). Each is 128 bytes and 64-byte aligned, with fixed-width little-endian fields, all accessed atomically. The scheduler can write the pages it maps, but the nucleus never reads a record back, so a scheduler that overwrites one only misleads itself.

**Scheduler record (slot 0)**

| Offset | Field |
|---:|---|
| 0 | `sequence` (u64) |
| 8 | `layout` (u32, 1), `record_size` (u32, 128) |
| 16 | `events` (u64): events published, equal to the bound EventCount |
| 24 | `timer_expirations` (u64) |
| 32 | `agent_state` (u32), `flags` (u32) |

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

- **Flags:** bit 0 `AWAITING_BUDGET` (runnable, but its Time is exhausted), bit 1 `HAS_TIME`. Without a bound Time `budget_remaining_ns` is `u64::MAX`.
- **State:** `Inactive` for a Thread that is not started, `Runnable` for a queued Thread and one waiting for budget, `Running`, `Blocked`, `Faulted`, and `Dying` once the Thread is retired.
- **Block reason:** none, Notification wait, EventCount await, protected procedure call.
- **Stop reason:** blocked, slice end, budget exhausted, yielded, replaced, exited, faulted.
- **`cpu`:** the CPU the Thread runs on, or `u32::MAX`.
- **Freshness:** the counters are as of the last published change, not of the moment you read them; `Time.Query` gives the exact budget.

The nucleus is the only writer. It makes `sequence` odd, writes the fields, then makes it even with release ordering. A reader loads `sequence` with acquire ordering, retries while it is odd, reads the fields, and retries if `sequence` changed; `ThreadRecord::try_snapshot` and `SchedulerRecord::try_snapshot` do this. `sequence` only detects torn reads; `generation` counts the Thread's state changes.

## Implementation

The root Scheduler is the only Scheduler; `Untyped.Retype` cannot create one. Started Threads join the kernel's first-in, first-out run queue.

The nucleus publishes a Thread's record when it starts, queues, waits for budget, runs, blocks, stops, faults or is retired, and when `AssignRecord` links it. Retiring a Thread publishes `Dying` with its incarnation and drops the slot, and the scheduler may reuse the slot after it sees that. A shared Frame must stay mapped and must not be retired while the table uses it; the nucleus does not reclaim Frame memory.

## Planned changes

Scheduler derivation and handing Threads to subordinate schedulers, a dispatch operation with scheduling events and activation, and a timer a scheduler can arm.
