# Invocation

| | |
|---|---|
| Wire type | `0x07` (core) |
| Target | An exported component API entry point in an `AddressSpace` |
| Status | Capability construction is active; `Call` dispatch and PPC execution are not implemented |

## Purpose

An `Invocation` capability identifies an entry point in a component API within
a target `AddressSpace` incarnation. It does not name a server `Thread`; the
caller's `Thread` migrates into the target `AddressSpace`. Any component with
`GRANT` authority on the target `AddressSpace` and `INSTALL` authority on a
destination `KeyTable` can construct an `Invocation` through
`AddressSpace.CreateInvocation`. The operation stores the supplied function
address without construction-time mapping or executable validation; a zero
address is rejected with `InvalidPointer` because the payload's absent form
belongs to the fixed return key, which only the kernel constructs. No
component-interface registry or additional function-pointer validation is
required. A component loader may parse interface specifications and
prepare/distribute exports as setup policy, installing them in a `KeyTable` it
selects: the direct recipient's table or a namespace-like component's table for
API discovery and joining. Return is the `Invocation.Return` operation on a
fixed kernel-installed return key — an Invocation capability at well-known
Slot(1) whose entry is absent, accepting only `Return`. Its presence in a
KeyTable is a libOS composition invariant, not a kernel guarantee. Overall call
lifecycle behavior remains unspecified.

Queued rendezvous is userspace composition over Invocation and
`Notification`/`EventCount`; it is not a kernel object kind.

## User-level visible operations

| Op | Name | Status |
|---|---|---|
| `0` | Call | `x0` Invocation capability, `x1` operation ID `0`, `x2..x7` six `u64` arguments forwarded to the interface function; return mapping remains open. Call dispatch is not implemented. |
| `1` | Return | Invoked on the fixed kernel-installed return key (a well-known KeySlot, empty function address, no target), which accepts only `Return` while normal Invocation capabilities accept only `Call`. Pops the invoking Thread's own continuation record and restores AddressSpace, SP, and PC, with return values in designated registers (mapping open). The return key resides at well-known Slot(1). Presence guarantee and validation rules (underflow, target liveness, frame skipping) remain open. Not implemented. |

## Contract details still to specify

Selected execution-context directions (canonical contract, communication
section): the kernel keeps one stack per core and a Thread's continuation
never lives on a kernel stack; each Thread carries a bounded kernel-owned
invocation stack of continuation records (target AddressSpace identity,
saved user SP/PC, return metadata) with call-time depth-exhaustion
rejection; the userspace entry stub provisions the target stack and the
kernel validates it; return is the `Invocation.Return` operation on the
fixed kernel-installed return key, popping the Thread's own record and
restoring AddressSpace, SP, and PC with return values in designated
registers.

Still to specify:

- Invocation derivation, rights attenuation, and caller badge semantics.
- The exact invocation-stack record layout, depth, and identifiers, and
  nested/concurrent-call and bounded-resource rules.
- The `Return` validation rules (underflow, target liveness, frame skipping).
- Return-value mapping, pointer/shared-memory rules, and whether capability
  transfer is supported.
- Return, fault, cancellation, and Thread-teardown outcomes.

The six SVC argument words are forwarded as six `u64` interface-function
arguments. The mapping from the function's return values to the SVC result words
remains open.

## Implementation status

The shared `CoreType` catalogue recognizes Invocation at ID 7. `AddressSpace.CreateInvocation` constructs and installs CALL-only Invocation capabilities with a checked target AddressSpace identity and supplied function address; the address is an optional payload field (`Option<NonZero<u64>>`) whose absent form only the kernel builds, so a zero address is rejected with `InvalidPointer`. Kickstart installs the entry-absent return key at well-known Slot(1). There is no `Call` wrapper or kernel handler, context migration, or return path; a created Invocation cannot yet be called, and `Return` on the return key is not dispatched yet.
