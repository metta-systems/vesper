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

Adds `x2` (nonzero) to the counter and wakes every Thread waiting for a target the new value has reached. If the sum would exceed `u64::MAX`, Advance fails with `CounterOverflow` (status 31), the counter stays unchanged, and every waiting Thread also resumes with `CounterOverflow`.

### Await

Returns at once if the counter is already at least `x2`; otherwise blocks until an Advance reaches it. `x3` is the timeout; only `u64::MAX` (wait forever, `EventCountKey::WAIT_INFINITE`) is accepted.

### Read

Returns the current value without blocking. `EventCountReader` in `libs/object` tracks a reader's own position on top of Read and Await.

### Errors

| Error | Cause |
|---|---|
| `InsufficientRights` | Missing `Send` or `Receive` |
| `InvalidOperation` | Delta 0, a timeout other than `u64::MAX`, or a nonzero unused argument |
| `CounterOverflow` | The advance would exceed `u64::MAX` |
| `PoolExhausted` | Too many Threads already waiting on this EventCount |
| `TypeMismatch` | The key does not name an EventCount |

## Implementation

Data written to shared memory before an Advance is visible to a Thread that observes the new value through Await or Read. Retiring a waiting Thread removes it from the queue.

## Planned changes

Finite timeouts for Await, once the kernel has a clock.

An [IRQHandler](irq_handler.md) target, as an alternative to a Notification: each interrupt advances the count, so a Thread sleeping on it learns from one wakeup how many events arrived ([plan](../../capabilities-implementation-plan.md#interrupt-delivery)).
