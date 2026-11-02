# Invocation

| | |
|---|---|
| Wire type | `0x06` (core) |
| Backing | Stored in the capability: target AddressSpace, entry address, stack pool handle, key-placement window |
| Operations | Call, CallWithKey |

## Purpose

An Invocation is a callable entry point in another component. Calling it moves the calling Thread into the target AddressSpace, where it runs the entry function on a stack the kernel takes from one of the target's [stack pools](../arch/address_space.md#stack-pools); [`Thread.Return`](thread.md#return) brings it back with two result words. No server Thread is involved: the caller's own Thread executes the target code, using the target's KeyTable while it is there.

Invocations are created by [`AddressSpace.CreateInvocation`](../arch/address_space.md#createinvocation) and carry the `Call` and `Badge` rights. [`KeyTable.CopyDerive`](key_table.md#copyderive) can copy one with fewer rights, so a holder that does not need to mint copies it with `Badge` masked off, and, from a key that carries `Badge`, set a badge on the copy; the exporter receives the badge in `x1` of its entry and uses it to tell its callers apart. Request queues and server Threads can be built in userspace on top of Invocation, Notification and EventCount; `kernel/tests/endpoint-test` shows one.

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Call | `x2..x7` six arguments | `Call` | `x1` `r0`, `x2` `r1` returned by the target |
| 1 | CallWithKey | As Call, plus `x8` the key to lend | `Call` on the Invocation, `Lend` on the lent key | As Call |

Any other operation fails with `InvalidOperation`.

### Call

The caller passes six `u64` arguments in `x2..x7`; `x9` is clobbered. The kernel takes the lowest free stack of the Invocation's pool and the Thread runs the target entry on it with the register state below. When the target calls `Thread.Return(r0, r1)`, the caller resumes after its `svc` with `x0 = 0`, `x1 = r0`, `x2 = r1`.

A call can nest: code running inside a target can call further Invocations, up to 16 levels deep. A call at depth 16 fails with `NestingDepth` (`x1` = 16).

### CallWithKey

`CallWithKey` behaves like `Call` and also lends one key, named by the caller in `x8`, to the target for the duration of the call. Its inputs are `x2..x7` the six arguments and `x8` the key to lend (a key in the caller's own table). The target entry receives the lent key in `x0`, installed in the target's own KeyTable, and `x1` stays zero; `x2..x7` are unchanged. A generated entry has the signature `fn(lent_key, _dummy, a0..a5)`.

**Accepting keys.** The exporter declares a key-placement window when it creates the Invocation: `x8` of [`CreateInvocation`](../arch/address_space.md#createinvocation) names `count` consecutive slots of the target's KeyTable starting at `first`. An Invocation created with count 0 accepts no key, and `CallWithKey` on it fails with `KeyNotAccepted`. The kernel places the lent key in the first vacant slot of the window that can still issue an incarnation; the caller never chooses the slot, and no occupied slot is overwritten. The window size bounds the number of simultaneous `CallWithKey` calls into the Invocation; when every slot is taken the call fails with `NoPlacementSlot`. The kernel does not reserve the window, so the exporter keeps it vacant.

**The lent key.** A key can be lent when it names a `KeyTable`, `Frame` or `DebugConsole` capability and carries the `Lend` right (rights bit 6, the same position for every kind). The lender keeps its own key. The borrowed entry names the same object with the same payload and exactly the same rights, including `Lend`; a caller that wants to lend less derives an attenuated key first with [CopyDerive](key_table.md#copyderive). The kernel marks the borrowed entry as lent. The target can invoke it, use it as a destination table for [CopyDerive](key_table.md#copyderive) or `Untyped.Retype`, delete it, and pass it as the lent key of a further `CallWithKey`, which hands a key down a call chain without copies. A lent entry is never the source of CopyDerive or Move (`InvalidOperation`), so the target cannot keep it. Every Thread of the target AddressSpace can use the borrowed entry while the call lasts.

**Cleanup.** `Thread.Return` removes the borrowed entry before the caller resumes. A target that already deleted it, or that put a key of its own into the freed slot, leaves nothing to remove: the slot's incarnation changed. [`Thread.Retire`](thread.md#retire) and a Thread parked after an unhandled fault remove the borrowed entries of all its outstanding calls. Revoking the lent key's origin during the call makes the borrowed entry fail with `InconsistentKey`; Return still completes.

| Error | Cause |
|---|---|
| `KeyNotAccepted` | The Invocation's placement window is empty |
| `InvalidKey`, `InconsistentKey` | The key in `x8` failed lookup; the details name argument register 8 (see [key errors](../README.md#key-errors)) |
| `InvalidObjectType` | The key in `x8` names a kind that cannot be lent |
| `InsufficientRights` | The key in `x8` lacks `Lend` (or the Invocation key lacks `Call`) |
| `StaleGeneration`, `NoFreeStack`, `NestingDepth` | As for `Call` |
| `NoPlacementSlot` | No slot of the placement window is vacant |
| `NotBound` | The target AddressSpace cannot receive keys: its KeyTable does not hold a capability to itself in slot 4 |

All rejections happen before migration and leave both tables and the Thread unchanged.

## Stacks

Each Invocation names a [stack pool](../arch/address_space.md#stack-pools) of its target AddressSpace. A Call takes the lowest free stack of the pool and enters the target with `SP` at the stack's end; `Thread.Return` and [`Thread.Retire`](thread.md#retire) give it back. The kernel does not touch the stack memory: the target maps the stacks, and guard pages around them.

| Error | Cause |
|---|---|
| `NoFreeStack` | Every stack of the pool is in use; `x1` is the pool's stack count |
| `StaleGeneration` | The pool was destroyed; `x1` is the slot's current generation |

A call back into the same pool (A to B to A) takes another stack, so a pool holds as many stacks as the deepest simultaneous use needs. The pool of a fault handler's Invocation is reserved for fault delivery when no other Invocation names it.

## Check order

Each operation stops at the first failing check and changes nothing:

- **CreateInvocation:** keys, kinds and rights; the target AddressSpace is live (`InconsistentKey`, `ObjectRetired`) → reserved operand bits (`InvalidOperation`) → entry address is nonzero (`InvalidPointer`) → placement window within the target's table (`InvalidSlot`) → destination slot.
- **Call:** key and `Call` right; the target AddressSpace is live (`InconsistentKey`, `ObjectRetired`) → the pool is live and has a free stack → the target's translation root and ASID are ready → invocation depth.
- **CallWithKey:** key and `Call` right; the target AddressSpace is live → the Invocation accepts keys (`KeyNotAccepted`) → the lent key resolves, can be lent (`InvalidObjectType`) and carries `Lend` (`InsufficientRights`) → the pool is live and has a free stack → translation readiness → invocation depth → a vacant placement slot (`NoPlacementSlot`).

## Register state

| State | Target entry after Call | Caller after Return |
|---|---|---|
| `x0` | Zero (unused); after CallWithKey the borrowed key | 0 / `r0` |
| `x1` | The badge of the invoked key; zero for an unbadged key | 0 / `r1` |
| `x2` | First argument | `r1` |
| `x3..x7` | Remaining arguments | Zero |
| `x8..x18` | Zero (including `x8`, the lent key of CallWithKey, and `x9`) | Zero |
| `x19..x30` | Zero | As before the Call |
| NZCV | Zero | As before the Call |
| Exception level, interrupt masks | As in the caller | As before the Call |
| `TPIDR_EL0` | Zero | As before the Call |
| SP, PC | End of the pool's stack, entry address | As before the Call, PC after the `svc` |

A rejected Call or Return is an ordinary error: the registers follow the usual [invocation rules](../README.md#registers) and the Thread stays where it was.

## Userspace support

- `InvocationKey::call(args)`, `InvocationKey::call_with_key(lent_key, args)` and `ThreadReturnKey::return_from_invocation(r0, r1)` in `libs/object` wrap Call, CallWithKey and Return. The raw `libsyscall::ppc_call`, `ppc_call_with_key` and `ppc_return` declare `x0..x18` as clobbered. `AddressSpaceKey::create_invocation_with_window` declares the placement window; `create_invocation` creates an Invocation that accepts no key.
- `ppc_export!(entry => body)` generates an entry function to pass to CreateInvocation. The entry takes eight `u64` arguments (two unused, then the six call arguments) and calls `body` with them. `body` returns `PpcResult { r0, r1 }`, which the entry hands to Return using the key recorded by `export::init_return_key`. If Return is rejected, the entry calls `vesper_thread_return_fault(status, detail1, detail2, r0, r1)`, which the component image must provide. For a CallWithKey target the first of the two unused arguments is the borrowed key.

## Implementation

Each Thread holds a 16-entry stack of call records (source AddressSpace, return PC and SP, flags, `x19..x30`, `TPIDR_EL0`, the slot and incarnation of a key lent by CallWithKey, and the pool slot and index of the stack the call took), 176 bytes each, so the kernel allocates nothing per call. The entry address is stored as given; the kernel does not check that it is mapped or executable. A fault while running inside a target goes to the target AddressSpace's fault handler. A Thread may block on a Notification or EventCount while inside a target.

## Planned changes

The two-word `PpcResult` return convention may still change.
