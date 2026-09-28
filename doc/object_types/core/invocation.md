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
address without construction-time mapping or executable validation; no
component-interface registry or additional function-pointer validation is
required. A component loader may parse interface specifications and
prepare/distribute exports as setup policy, installing them in a `KeyTable` it
selects: the direct recipient's table or a namespace-like component's table for
API discovery and joining. Return and overall call behavior remain unspecified.

Queued rendezvous is userspace composition over Invocation and
`Notification`/`EventCount`; it is not a kernel object kind.

## User-level visible operations

| Op | Name | Status |
|---|---|---|
| `0` | Call | `x0` Invocation capability, `x1` operation ID `0`, `x2..x7` six `u64` arguments forwarded to the interface function; return mapping remains open. Call dispatch is not implemented. |

## Contract details still to specify

- Invocation derivation, rights attenuation, and caller badge semantics.
- How the caller's execution context and stack are established in the target
  AddressSpace, including nested/concurrent calls and bounded resource use.
- Return-value mapping, pointer/shared-memory rules, and whether capability
  transfer is supported.
- Return, fault, cancellation, and Thread-teardown outcomes.

The six SVC argument words are forwarded as six `u64` interface-function
arguments. The mapping from the function's return values to the SVC result words
remains open.

## Implementation status

The shared `CoreType` catalogue recognizes Invocation at ID 7. `AddressSpace.CreateInvocation` constructs and installs CALL-only Invocation capabilities with a checked target AddressSpace identity and supplied function address. There is no `Call` wrapper or kernel handler, context migration, or return path; a created Invocation cannot yet be called.
