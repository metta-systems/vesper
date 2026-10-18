# Time

| | |
|---|---|
| Wire type | `0x04` (core) |
| Backing | Kernel object carved from an Untyped by `Untyped.Retype` |
| Operations | Transfer, Merge, Query |

## Purpose

A Time holds a CPU-time budget on one core. A scheduler gives budget to a child scheduler or to a Thread's Time by transferring part of its own. Nothing returns budget automatically: unused budget goes back only by an explicit Transfer or Merge. A Thread that waits for events, such as an interrupt [receiver](irq_handler.md), has its own Time.

Budgets are in nanoseconds. `u64::MAX` (`libobject::time::INFINITE_BUDGET`) is an infinite budget, which never decreases.

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Transfer | `x2` destination Time, `x3` amount | `Write` on both Times | — |
| 1 | Merge | `x2` source Time | `Write` on both Times | — |
| 2 | Query | — | `Read` | `x1` budget, `x2` zero |

`Read` is rights bit 0 and `Write` is bit 1. All unused argument registers must be zero.

### Transfer

Invoked on the source Time. Moves `x3` nanoseconds into the destination Time named by `x2`.

- A finite source loses the amount. An infinite source stays infinite.
- An amount of `u64::MAX` makes the destination infinite; only an infinite source can send it.
- An infinite destination stays infinite.
- A finite destination cannot reach `u64::MAX`: a sum that would is rejected.

### Merge

Invoked on the destination Time. Moves all of the budget of the source Time named by `x2` into the invoked Time and leaves the source empty with zero budget. An infinite source makes the destination infinite. The emptied source stays in its slot until it is deleted.

### Query

Returns the remaining budget in `x1`, rounded down to a whole nanosecond; `x2` is zero.

### Errors

| Error | Cause |
|---|---|
| `InsufficientBudget` (36) | A finite source holds less than the amount; `x1` is its remaining budget |
| `InvalidOperation` | Amount 0, the same Time as both operands, Times on different cores, a nonzero unused argument, a finite destination sum reaching `u64::MAX`, or budget taken out of a Time bound to a live Thread |
| `InsufficientRights` | Missing `Write` on either Time, or `Read` for Query |
| `TypeMismatch` | A key does not name a Time |

Every failure leaves both Times unchanged.

## Implementation

A Time bound to a Thread by [`Scheduler.ConfigureThread`](scheduler.md#configurethread) is that Thread's own while the Thread is live: Transfer and Merge can add budget to it but cannot take any out, and no other Thread can be bound to it. Once the Thread is retired the Time can be drained or bound again.

`Untyped.Retype` creates an empty Time on the creating core, carved from the Untyped's memory.

The first component receives the boot Time at `KeySlot::BOOT_TIME` (17): core 0 and an infinite budget.

The nucleus charges the running Thread's bound Time with the counter ticks it executes, including the time it spends in system calls. The charge is kept exactly and converted to nanoseconds, rounded up, only when it is compared with the budget, so a Thread never runs past its budget and Query reports the remaining budget rounded down. Time spent handling interrupts is not charged. An infinite budget is never charged.

A Thread whose Time is exhausted stops and is not run again: it is published as waiting for budget. A Transfer or Merge that brings budget into the Time ends the wait and announces the Thread to its dispatcher. A Thread with no bound Time, such as the boot Thread, has no limit; the nucleus still measures how long it ran.

The nucleus runs no Thread by itself: a Scheduler's dispatcher decides which Thread runs, on which Time and for how long (a dispatch limit). The EL1 physical timer is one-shot: it is programmed for the earliest of the dispatch limit, the moment the running Thread's budget runs out, and it is off when neither applies.

## Planned changes

None yet.
