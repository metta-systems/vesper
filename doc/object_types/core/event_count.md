# EventCount

| | |
|---|---|
| Wire type | `0x08` (core) |
| Backing | Kernel object carved from an Untyped by `Untyped.Retype` |
| Operations | Advance, Await, Read |

## Purpose

An EventCount is a 64-bit counter that only increases. Producers advance it; consumers wait until it reaches a value or read it. Each consumer keeps its own position, and reading or waiting does not change the counter, so any number of readers can follow the same producer. Typical uses are ring buffers (counting produced and consumed slots) and progress tracking. For one-shot signals, use a [Notification](notification.md).

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Advance | `x2` delta | `Send` | `x1` the new value |
| 1 | Await | `x2` target, `x3` timeout | `Receive` | `x1` the current value |
| 2 | Read | — | `Receive` | `x1` the current value |

`Send` is rights bit 1 and `Receive` is bit 0.

### Advance

Adds `x2` (nonzero) to the counter and wakes every Thread waiting for a target the new value has reached. If the sum would exceed `u64::MAX`, Advance fails with `CounterOverflow` (status 32), the counter stays unchanged, and every waiting Thread also resumes with `CounterOverflow`.

### Await

Returns at once if the counter is already at least `x2`; otherwise blocks until an Advance reaches it. `x3` is the timeout in nanoseconds from now: `u64::MAX` (`EventCountKey::WAIT_INFINITE`) waits forever and 0 is invalid. A finite timeout ends the wait with `TimedOut` at the first timer interrupt at or after the deadline (the timeout rounded up to counter ticks); `x1` is then the counter value. An Advance that reaches the target first wins and the deadline is forgotten. `EventCountKey::sleep(ns)` is a wait on an EventCount nobody advances, for a target it cannot reach, that treats `TimedOut` as success.

### Read

Returns the current value without blocking. `EventCountReader` in `libs/object` tracks a reader's own position on top of Read and Await.

### Errors

| Error | Cause |
|---|---|
| `InsufficientRights` | Missing `Send` or `Receive` |
| `InvalidOperation` | Delta 0, a timeout of 0, or a nonzero unused argument |
| `TimedOut` | The timeout ended the wait (`x1` the counter value) |
| `CounterOverflow` | The advance would exceed `u64::MAX` |
| `PoolExhausted` | Too many Threads already waiting on this EventCount |
| `TypeMismatch` | The key does not name an EventCount |

## Implementation

Data written to shared memory before an Advance is visible to a Thread that observes the new value through Await or Read. Retiring a waiting Thread removes it from the queue.

## Planned changes

An [IRQHandler](irq_handler.md) target, as an alternative to a Notification: each interrupt advances the count, so a Thread sleeping on it learns from one wakeup how many events arrived ([plan](../../agentic/capabilities-implementation-plan.md#interrupt-delivery)).
