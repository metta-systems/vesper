# Notification

| | |
|---|---|
| Wire type | `0x07` (core) |
| Backing | Kernel object carved from an Untyped by `Untyped.Retype` |
| Operations | Signal, Await, Poll |

## Purpose

A Notification is a 64-bit set of pending signal bits. Signalling ORs bits into the set; repeated signals of the same bit merge into one. A waiter takes all pending bits at once and clears them. Use it to report events such as completions or interrupts, or to wake a worker to look at a shared queue. When several readers must each see every update, use an [EventCount](event_count.md).

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Signal | `x2` bits | `Send` | — |
| 1 | Await | `x2` timeout | `Recv` | `x1` the pending bits |
| 2 | Poll | — | `Recv` | `x1` the pending bits, or 0 |

`Send` is rights bit 1 and `Recv` is bit 0.

### Signal

ORs bits into the Notification. A capability with a nonzero badge signals its badge and ignores `x2`; a capability with badge zero signals `x2`. If a Thread is waiting, the oldest waiter receives all pending bits and resumes; the others keep waiting.

### Await

Returns and clears all pending bits. If none are pending, the caller blocks until a Signal arrives. `x2` is the timeout in nanoseconds from now: `u64::MAX` (`NotificationKey::WAIT_INFINITE`) waits forever and 0 is invalid. A finite timeout ends the wait with `TimedOut` at the first timer interrupt at or after the deadline; `x1` is then 0, because a wait that timed out consumed nothing. A Signal that arrives first wins and the deadline is forgotten.

### Poll

Returns and clears all pending bits without blocking; `x1` is 0 if none were pending.

### Errors

| Error | Cause |
|---|---|
| `InsufficientRights` | Missing `Send` or `Recv` |
| `InvalidOperation` | A timeout of 0, or a nonzero unused argument |
| `TimedOut` | The timeout ended the wait (`x1` always 0) |
| `PoolExhausted` | Too many Threads already waiting on this Notification |
| `TypeMismatch` | The key does not name a Notification |

## Implementation

Bits written to shared memory before a Signal are visible to the Thread that receives those bits through Await or Poll. Retiring a waiting Thread removes it from the queue.

## Planned changes

None yet.
