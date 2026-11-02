# WaitSet

| | |
|---|---|
| Wire type | `0x09` (core) |
| Backing | Kernel object carved from an Untyped by `Untyped.Retype` |
| Operations | AddMember, UpdateMember, RemoveMember, Await |

## Purpose

A WaitSet lets one Thread wait for several [Notifications](notification.md) and [EventCounts](event_count.md) at once. It holds up to 8 members, each in a numbered slot with one condition: a Notification member is ready while any bit of its mask is pending, an EventCount member while its counter has reached its target. Await returns the mask of ready slots (bit *n* for slot *n*) and consumes nothing; the caller then polls or reads the ready members itself.

Readiness is level-triggered. A condition that already holds when Await is called counts, and it keeps counting until the member changes. A set has one waiter at a time.

**A set must be the only claimant of its Notifications.** A Notification that a set lists refuses a direct `Await` (`Watched`), and a Notification with a Thread already waiting on it cannot join a set. Otherwise the direct waiter would take each signal's bits and the set would never see them. See [Notification](notification.md#lists-in-a-waitset). `Poll` stays allowed, so the Thread that owns the set takes the bits of a ready member with it.

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | AddMember | `x2` member key, `x3` slot, `x4` condition | `Write`, and `Receive` on the member | — |
| 1 | UpdateMember | `x2` slot, `x3` condition | `Write` | — |
| 2 | RemoveMember | `x2` slot | `Write` | — |
| 3 | Await | `x2` timeout | `Receive` | `x1` the mask of ready slots |

`Write` is rights bit 1 and `Receive` is bit 0.

### AddMember

Puts a Notification or an EventCount in the vacant slot `x3` (0 to 7). `x2` is the member's key in the caller's table. `x4` is the condition: for a Notification the bits to watch (nonzero), for an EventCount the target its value must reach (0 is always ready). Adding a Notification marks it as watched, which refuses direct waits on it; it fails with `Watched` if a Thread is already waiting on it directly, and the slot stays vacant. If a Thread is already waiting on the set and the new member is ready, the wait completes.

### UpdateMember

Replaces the condition of the member in slot `x2` and keeps the member. A Notification mask must stay nonzero. If a Thread is waiting and the new condition holds, the wait completes.

### RemoveMember

Empties slot `x2`. A Notification not listed in any WaitSets any more can be awaited directly again.

### Await

Returns the mask of ready slots at once if any member is ready. A member whose object is no longer live is an error instead (`MemberRetired`, `x1` the slot), reported ahead of any ready slot and until the member is removed. Otherwise the caller blocks until a Signal or Advance makes a member ready. `x2` is the timeout in nanoseconds from now: `u64::MAX` (`WaitSetKey::WAIT_INFINITE`) waits forever and 0 is invalid. A finite timeout ends the wait with `TimedOut` at the first timer interrupt at or after the deadline; `x1` is then 0. A set with no members waits for the timeout.

### Errors

| Error | Cause |
|---|---|
| `InsufficientRights` | Missing `Write` on the set, or `Receive` on the set or the member |
| `InvalidOperation` | A timeout of 0, a Notification mask of 0, or a nonzero unused argument |
| `InvalidSlot` | A slot above 7 |
| `SlotOccupied` | AddMember on a slot that holds a member |
| `EmptySlot` | UpdateMember or RemoveMember on a vacant slot |
| `TypeMismatch` | The key does not name a WaitSet, or a member key names something other than a Notification or EventCount |
| `TimedOut` | The timeout ended the wait |
| `Watched` | AddMember of a Notification that a Thread is already waiting on |
| `MemberRetired` | A member's object is no longer live; `x1` is its slot, also for a waiter woken by it |
| `PoolExhausted` | A Thread is already waiting on this set |

The slot errors name the member slot in their detail word.

## Implementation

A set registers nothing with its members. After every Signal, Advance, interrupt delivery and membership change, the kernel checks the sets that have a blocked waiter and completes the wait of any with a ready member. A woken waiter sees the state as of the wake-up; another Thread can consume a Notification's bits before the waiter polls it, so the waiter polls and, if nothing is pending, waits again.

A member whose object is no longer live is reported to the waiter with `MemberRetired`, naming the slot, and stays in its slot until it is removed. Notifications and EventCounts cannot be retired yet, so this does not occur today.

Retiring the waiting Thread removes it from the set.
