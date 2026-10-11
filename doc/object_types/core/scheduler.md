# Scheduler

| | |
|---|---|
| Wire type | `0x05` (core) |
| Backing | Kernel object carved from the boot Untyped by Kickstart |
| Operations | ConfigureThread, StartThread (on root Scheduler) |

## Purpose

A Scheduler capability is the authority to make Threads run. A Thread created by [`Untyped.Retype`](untyped.md#retype) is inert: it has no AddressSpace and no execution state, and it never runs. Only the root Scheduler turns it into a Thread that runs, by configuring it and then starting it. The first component receives the root Scheduler at `KeySlot::ROOT_SCHEDULER` (18).

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | ShareRegion | — | — | Not implemented: `InvalidOperation` |
| 1 | ConfigureThread | `x2` Thread, `x3` AddressSpace, `x4` entry, `x5` stack pointer, `x6` Time, `x7` argument | `Write` on the Scheduler, Thread and Time; `Map` on the AddressSpace | — |
| 2 | StartThread | `x2` Thread | `Write` on the Scheduler and Thread | — |

`Read` is rights bit 0, `Write` and `Map` are bit 1. `SchedulerKey` in `libs/object` wraps both operations.

### ConfigureThread

Binds the inert Thread `x2` to the AddressSpace `x3`, an EL0 start state (entry `x4`, stack pointer `x5`, `x0` set to `x7` on first entry) and its own Time `x6`. The Thread records the Time it runs on and the Scheduler that configured it, and the Time becomes the Thread's own: while the Thread is live no budget can be taken out of it and no other Thread can be bound to it (see [Time](time.md#implementation)). The caller keeps the Time capability. Nothing charges the Time yet.

| Error | Cause |
|---|---|
| `InvalidOperation` | The Thread is not inert (already configured or started); the entry is not 4-byte aligned or not below the end of the user address range; the stack pointer is not 16-byte aligned or above that end; the Time belongs to another core or is already bound to a live Thread |
| `NotMapped` | The AddressSpace has no translation root |
| `NoAsidAssigned` | The AddressSpace has no ASID |
| `InsufficientRights` | A missing right, or the Scheduler is not the root |
| `TypeMismatch` | A key names the wrong kind |

### StartThread

Makes the configured Thread `x2` runnable; it first runs its configured start state. `x3..x7` must be zero. A Thread that is inert or already started fails with `InvalidOperation`.

## Implementation

The root Scheduler is the only Scheduler; `Untyped.Retype` cannot create one. Started Threads join the kernel's first-in, first-out run queue.

## Planned changes

Scheduler derivation and handing Threads to subordinate schedulers, and `ShareRegion` for the scheduling records a scheduler shares with the kernel.
