# Time

| | |
|---|---|
| Wire type | `0x04` (core) |
| Backing | Kernel Time pool; created by `Untyped.Retype` |
| Status | Transfer, Merge, Query |

## Purpose

A Time holds a CPU-time budget on one core together with a priority chain: one priority for each scheduler the budget passed through. A scheduler gives budget to a child scheduler or to a Thread's Time by transferring part of its own, and the receiving Time records the priority the sender assigned. A Thread that waits for events, such as an interrupt [receiver](irq_handler.md), has its own Time.

Budgets are in nanoseconds. `u64::MAX` (`libobject::time::INFINITE_BUDGET`) is an infinite budget, which never decreases. A priority is a `u16` where a lower number is more urgent: `0` is the most urgent, and ordinary components start around `0x8000` (`DEFAULT_PRIORITY`).

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Transfer | `x2` destination Time, `x3` amount, `x4` priority | `WRITE` on both Times | — |
| 1 | Merge | `x2` source Time | `WRITE` on both Times | — |
| 2 | Query | — | `READ` | `x1` budget, `x2` priority |

`READ` is rights bit 0 and `WRITE` is bit 1.

### Transfer

Invoked on the source Time. Moves `x3` nanoseconds into the destination Time named by `x2`, which records priority `x4` for the source's scheduler.

- A finite source loses the amount. An infinite source stays infinite.
- An amount of `u64::MAX` makes the destination infinite; only an infinite source can send it.
- An infinite destination stays infinite.
- The destination's chain becomes the union of both chains, one entry per scheduler. A scheduler present in both keeps the numerically larger (less urgent) priority, so a Time never becomes more urgent by receiving budget. The source's entry is then set to `x4` and placed last.

### Merge

Invoked on the destination Time. Moves all of the budget of the source Time named by `x2` into the invoked Time and leaves the source empty: zero budget and an empty chain. The invoked Time keeps its own chain, so the merged budget runs at its priority, and returning budget to a parent never lowers the parent's priority. The source's chain is discarded, except that an invoked Time with an empty chain, such as a new one, takes the source's chain. An infinite source makes the destination infinite. The emptied source stays in its slot until it is deleted.

### Query

Returns the remaining budget in `x1` and, in `x2`, the priority at the last chain entry: the scheduler that most recently gave this Time budget. An empty chain reports `0xFFFF` (`LOWEST_PRIORITY`).

### Errors

| Error | Cause |
|---|---|
| `INSUFFICIENT_BUDGET` (35) | A finite source holds less than the amount; `x1` is its remaining budget |
| `PRIORITY_CHAIN_FULL` (36) | Transfer only: the destination's chain would exceed 16 entries |
| `INVALID_OPERATION` | Amount 0, priority above `0xFFFF`, the same Time as both operands, Times on different cores, a finite sum reaching `u64::MAX`, or a nonzero unused argument |
| `INSUFFICIENT_RIGHTS` | Missing `WRITE` on either Time, or `READ` for Query |
| `TYPE_MISMATCH` | A key does not name a Time |

Every failure leaves both Times unchanged.

## Implementation

`Untyped.Retype` creates an empty Time on the creating core. It takes no memory from the region; each Time takes a slot in a kernel pool created at boot.

The first component receives the boot Time at `KeySlot::BOOT_TIME` (17): core 0, a chain naming itself at priority `0`, and a budget of one hour of CPU time. A kernel built with the `infinite-boot-time` feature gives it an infinite budget instead.

Nothing charges a Time yet: the nucleus preempts with a fixed 10 ms tick from the EL1 physical timer and runs Threads in first-in, first-out order.

## Planned changes

Dispatch of a Thread on a Time. The fixed tick goes away: on each dispatch the nucleus charges the running Time and programs the timer for the earlier of its budget running out and the dispatch timeout. A wakeup preempts the running Thread when the woken Thread's Time has the more urgent priority.
