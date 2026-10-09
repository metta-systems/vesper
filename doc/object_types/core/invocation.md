# Invocation

| | |
|---|---|
| Wire type | `0x07` (core) |
| Backing | Stored in the capability: target AddressSpace, entry address, stack extent |
| Status | Call |

## Purpose

An Invocation is a callable entry point in another component. Calling it moves the calling Thread into the target AddressSpace, where it runs the entry function on a stack the target provides; [`Thread.Return`](thread.md#return) brings it back with two result words. No server Thread is involved: the caller's own Thread executes the target code, using the target's KeyTable while it is there.

Invocations are created by [`AddressSpace.CreateInvocation`](../arch/address_space.md#createinvocation) and carry only the `CALL` right. Request queues and server Threads can be built in userspace on top of Invocation, Notification and EventCount; `kernel/tests/endpoint-test` shows one.

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Call | `x2..x7` six arguments, `x9` target stack pointer | `CALL` | `x1` `r0`, `x2` `r1` returned by the target |

Any other operation fails with `INVALID_OPERATION`.

### Call

The caller passes six `u64` arguments in `x2..x7` and the stack pointer the target should start with in `x9`. The Thread then runs the target entry with the register state below. When the target calls `Thread.Return(r0, r1)`, the caller resumes after its `svc` with `x0 = 0`, `x1 = r0`, `x2 = r1`.

A call can nest: code running inside a target can call further Invocations, up to 16 levels deep. A call at depth 16 fails with `NESTING_DEPTH` (`x1` = 16).

## Stack extent

Each Invocation carries the stack extent `[base, end)` and minimum headroom `M` its target published at creation. `base`, `end` and `M` are multiples of 16, the extent lies in the user address range `[0, 2^48)`, and `0 < M <= end - base`.

The stack pointer passed in `x9` must be 16-byte aligned, with `base < SP <= end` and `SP - base >= M`. A fresh stack starts at `SP = end`. The kernel checks these numbers only; the target is responsible for mapping the stack and for guard pages around it.

### `InvalidStack`

Stack checks fail with `INVALID_STACK` (status 32), the offending value in `x1` and a reason in `x2`:

| Reason | Name | Condition | `x1` |
|---:|---|---|---|
| 1 | `ExtentEmpty` | `end == base` | `end` |
| 2 | `ExtentInverted` | `end < base` | `end` |
| 3 | `BaseOutsideUserRange` | `base` outside the user range | `base` |
| 4 | `EndOutsideUserRange` | `end` beyond the user range | `end` |
| 5 | `BaseMisaligned` | `base` not 16-byte aligned | `base` |
| 6 | `EndMisaligned` | `end` not 16-byte aligned | `end` |
| 7 | `MinimumHeadroomZero` | `M == 0` | `M` |
| 8 | `MinimumHeadroomMisaligned` | `M` not 16-byte aligned | `M` |
| 9 | `MinimumHeadroomTooLarge` | `M > end - base` | `M` |
| 10 | `SpMisaligned` | SP not 16-byte aligned | SP |
| 11 | `SpOutOfRange` | `SP <= base` or `SP > end` | SP |
| 12 | `SpInsufficientHeadroom` | `SP - base < M` | SP |

CreateInvocation checks reasons 1–9 and Call checks 10–12, each in table order, reporting the first that fails.

## Check order

Each operation stops at the first failing check and changes nothing:

- **CreateInvocation:** keys, kinds and rights; the target AddressSpace is live (`INCONSISTENT_KEY`, `ObjectRetired`) → entry address is nonzero (`INVALID_POINTER`) → stack extent → destination slot.
- **Call:** key and `CALL` right; the target AddressSpace is live (`INCONSISTENT_KEY`, `ObjectRetired`) → stack pointer → the target's translation root and ASID are ready → invocation depth.

## Register state

| State | Target entry after Call | Caller after Return |
|---|---|---|
| `x0`, `x1` | Zero (two unused arguments) | 0 / `r0` |
| `x2` | First argument | `r1` |
| `x3..x7` | Remaining arguments | Zero |
| `x8..x18` | Zero | Zero |
| `x19..x30` | Zero | As before the Call |
| NZCV | Zero | As before the Call |
| Exception level, interrupt masks | As in the caller | As before the Call |
| `TPIDR_EL0` | Zero | As before the Call |
| SP, PC | `x9`, entry address | As before the Call, PC after the `svc` |

A rejected Call or Return is an ordinary error: the registers follow the usual [invocation rules](../README.md#registers) and the Thread stays where it was.

## Userspace support

- `InvocationKey::call(args, target_sp)` and `ThreadReturnKey::return_from_invocation(r0, r1)` in `libs/object` wrap Call and Return. The raw `libsyscall::ppc_call` and `ppc_return` declare `x0..x18` as clobbered.
- `ppc_export!(entry => body)` generates an entry function to pass to CreateInvocation. The entry takes eight `u64` arguments (two unused, then the six call arguments) and calls `body` with them. `body` returns `PpcResult { r0, r1 }`, which the entry hands to Return using the key recorded by `export::init_return_key`. If Return is rejected, the entry calls `vesper_thread_return_fault(status, detail1, detail2, r0, r1)`, which the component image must provide.

## Implementation

Each Thread holds a 16-entry stack of call records (source AddressSpace, return PC and SP, flags, `x19..x30`, `TPIDR_EL0`), so the kernel allocates nothing per call. The entry address is stored as given; the kernel does not check that it is mapped or executable. A fault while running inside a target goes to the target AddressSpace's fault handler. A Thread may block on a Notification or EventCount while inside a target.

## Planned changes

The `x9` stack-pointer register and the two-word `PpcResult` return convention may still change.
