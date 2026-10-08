# Thread

| | |
|---|---|
| Wire type | `0x03` (core) |
| Backing | Kernel Thread pool, created at boot |
| Status | Return, Retire |

## Purpose

A Thread is a schedulable flow of execution. At any moment it runs in one AddressSpace — its own, or the target of an [Invocation](invocation.md) it has called into — and resolves keys in that AddressSpace's KeyTable.

A Thread capability takes one of two forms:

- **Named** — refers to a specific Thread and allows controlling it.
- **Return key** — refers to no particular Thread. It lets the invoking Thread return from its current [Invocation](invocation.md) call. Every AddressSpace has one at `KeySlot::THREAD_RETURN` (slot 1).

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Return | `x2` `r0`, `x3` `r1` | Return key | Does not return; the caller of the Invocation resumes |
| 1 | Grant | — | — | Not implemented: `INVALID_OPERATION` |
| 2 | Suspend | — | — | Not implemented: `INVALID_OPERATION` |
| 3 | Resume | — | — | Not implemented: `INVALID_OPERATION` |
| 4 | Retire | — | `RETIRE` on a named Thread | — |

### Return

Ends the current Invocation call and resumes the Thread in the AddressSpace it called from, with `x0 = 0`, `x1 = r0`, `x2 = r1`; see [Invocation](invocation.md#register-state) for the full register state. `x0` holds the key of slot 1 in the current AddressSpace's table, and `x4..x7` are ignored.

The key is the same for every Thread in the AddressSpace: `ThreadReturnKey::provisioned(guard, size_bits)` builds it from the table's guard and size, since slot 1 always holds incarnation 1. A component that deletes its slot 1 entry can no longer return.

| Situation | Outcome |
|---|---|
| Key fails lookup | Key error; the call is not ended |
| Return on a named Thread | `INVALID_OPERATION` |
| No call to return from | Fault, kind 1 (`IllegalReturn`) |
| The calling AddressSpace has been retired | Fault, kind 2 (`ReturnTargetRetired`) |

The two faults are reported at the Return's `svc` through [fault delivery](#fault-delivery); the call is not ended.

`ThreadReturnKey::return_from_invocation(r0, r1)` wraps Return. It returns `Result<Infallible, CapError>`: on success it does not return; a rejection is `Err`; a local `SUCCESS` status is reported as `CapError::UnexpectedReturn` (status 34).

### Retire

Tears down a named Thread other than the caller: cancels its pending waits, removes it from the run queue and frees its slot in the Thread pool. Later invocations through any capability to it fail. The Thread's AddressSpace is unaffected; retire it separately with [`AddressSpace.Retire`](../arch/address_space.md#retire). Retire is also how a `Faulted` Thread is removed.

| Error | Cause |
|---|---|
| `INSUFFICIENT_RIGHTS` | The capability lacks `RETIRE` |
| `INVALID_OPERATION` | The target is the calling Thread, the capability is the return key, or an argument is nonzero |

## Fault delivery

A synchronous exception from EL0 other than `svc` (data or instruction abort, alignment fault, undefined instruction, FP/SIMD use, `brk` and the like), or a failed Return, is a fault. The kernel delivers it by making the faulting Thread itself call the Invocation at `KeySlot::FAULT_HANDLER` (slot 16) of the AddressSpace it is running in. The handler runs on the faulting Thread, with SP at the top of the Invocation's stack extent, and receives:

| Register | Contents |
|---|---|
| `x2` | Fault kind (`FaultKind`): 0 CPU exception, 1 `IllegalReturn`, 2 `ReturnTargetRetired` |
| `x3` | `ESR_EL1` (0 for a Return fault) |
| `x4` | `FAR_EL1` (0 for a Return fault) |
| `x5` | Faulting PC |
| `x6` | Faulting SP |
| `x7` | Invocation depth at the fault |

The handler finishes with an ordinary `Thread.Return`; `r0` selects what happens next (`FaultAction`):

| `r0` | Action |
|---|---|
| 0 | Retry: resume the faulting instruction with the saved state |
| 1 | Skip: resume at the next instruction |
| 2 or any other value | Terminate: the Thread becomes `Faulted` |

After a Return fault, use skip or terminate; retry repeats the failing Return.

A fault is unhandled when slot 16 holds no usable Invocation, when this AddressSpace's handler is already handling another fault, when the Thread faults again inside its own handler, or when the forced call itself fails (for example, the invocation stack is full). An unhandled fault leaves the Thread `Faulted`: it never runs again, and the kernel runs the next Thread. A `Faulted` Thread is removed with Retire.

## Implementation

Threads are created by the boot code; `Untyped.Retype` cannot create them. A Thread starts either at EL0 or, for trusted boot code, at EL1 using `SP_EL0`. Its saved state (all general registers, SP, PC, flags and `TPIDR_EL0`) lives in the Thread, so a blocked Thread holds no kernel stack. Each Thread has room for 16 nested Invocation calls.

At EL0, a Thread can read the virtual counter (`CNTVCT_EL0`, `CNTFRQ_EL0`) and its own `TPIDR_EL0`; `TPIDRRO_EL0` reads as zero. The physical counter, timers, performance monitors, the debug communications channel and FP/SIMD trap as faults.
