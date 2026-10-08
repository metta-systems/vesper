# Notification

| | |
|---|---|
| Wire type | `0x08` (core) |
| Backing | Kernel Notification pool; created by `Untyped.Retype` |
| Status | Signal, Wait, Poll |

## Purpose

A Notification is a 64-bit set of pending signal bits. Signalling ORs bits into the set; repeated signals of the same bit merge into one. A waiter takes all pending bits at once and clears them. Use it to report events such as completions or interrupts, or to wake a worker to look at a shared queue. When several readers must each see every update, use an [EventCount](event_count.md).

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Signal | `x2` bits | `SEND` | — |
| 1 | Wait | `x2` timeout | `RECV` | `x1` the pending bits |
| 2 | Poll | — | `RECV` | `x1` the pending bits, or 0 |

`SEND` is rights bit 1 and `RECV` is bit 0.

### Signal

ORs bits into the Notification. A capability with a nonzero badge signals its badge and ignores `x2`; a capability with badge zero signals `x2`. If a Thread is waiting, the oldest waiter receives all pending bits and resumes; the others keep waiting.

### Wait

Returns and clears all pending bits. If none are pending, the caller blocks until a Signal arrives. `x2` is the timeout; only `u64::MAX` (wait forever, `NotificationKey::WAIT_INFINITE`) is accepted.

### Poll

Returns and clears all pending bits without blocking; `x1` is 0 if none were pending.

### Errors

| Error | Cause |
|---|---|
| `INSUFFICIENT_RIGHTS` | Missing `SEND` or `RECV` |
| `INVALID_OPERATION` | A timeout other than `u64::MAX`, or a nonzero unused argument |
| `POOL_EXHAUSTED` | Too many Threads already waiting on this Notification |
| `TYPE_MISMATCH` | The key does not name a Notification |

## Implementation

Bits written to shared memory before a Signal are visible to the Thread that receives those bits through Wait or Poll. Retiring a waiting Thread removes it from the queue.

## Planned changes

Finite timeouts for Wait, once the kernel has a clock.
