# Notification

| | |
|---|---|
| Wire type | `0x07` (core) |
| Backing | Kernel object carved from an Untyped by `Untyped.Retype` |
| Operations | Signal, Wait, Poll |

## Purpose

A Notification is a 64-bit set of pending signal bits. Signalling ORs bits into the set; repeated signals of the same bit merge into one. A waiter takes all pending bits at once and clears them. Use it to report events such as completions or interrupts, or to wake a worker to look at a shared queue. When several readers must each see every update, use an [EventCount](event_count.md).

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Signal | `x2` bits | `Send` | — |
| 1 | Wait | `x2` timeout | `Recv` | `x1` the pending bits |
| 2 | Poll | — | `Recv` | `x1` the pending bits, or 0 |

`Send` is rights bit 1 and `Recv` is bit 0.

### Signal

ORs bits into the Notification. A capability with a nonzero badge signals its badge and ignores `x2`; a capability with badge zero signals `x2`. If a Thread is waiting, the oldest waiter receives all pending bits and resumes; the others keep waiting.

### Wait

Returns and clears all pending bits. If none are pending, the caller blocks until a Signal arrives. `x2` is the timeout; only `u64::MAX` (wait forever, `NotificationKey::WAIT_INFINITE`) is accepted.

### Poll

Returns and clears all pending bits without blocking; `x1` is 0 if none were pending.

### Errors

| Error | Cause |
|---|---|
| `InsufficientRights` | Missing `Send` or `Recv` |
| `InvalidOperation` | A timeout other than `u64::MAX`, or a nonzero unused argument |
| `PoolExhausted` | Too many Threads already waiting on this Notification |
| `TypeMismatch` | The key does not name a Notification |

## Implementation

Bits written to shared memory before a Signal are visible to the Thread that receives those bits through Wait or Poll. Retiring a waiting Thread removes it from the queue.

## Planned changes

Finite timeouts for Wait, once the kernel has a clock.
