# Invocation

| | |
|---|---|
| Wire type | `0x07` (core) |
| Target | An exported component API entry point in an `AddressSpace` |
| Status | Contract selected: Protected Procedure Call (PPC); not implemented |

## Purpose

An `Invocation` capability identifies an entry point in a component API within
a target `AddressSpace` incarnation. It does not name a server `Thread`; the
caller's Thread migrates into the target AddressSpace. Any component with management authority to both the target AddressSpace and
destination KeyTable can construct an Invocation from those capabilities and
the function pointer; no loader-only registration is required. Construction
requires no extra function-pointer or entry validation.
A component loader may parse interface specifications and prepare/distribute
exports as setup policy, installing them in a KeyTable it selects: the direct
recipient's table or a namespace-like component's table for API discovery and
joining. Entry validation, concrete construction/install rights, return, and
overall call behavior remain unspecified.

Queued rendezvous is userspace composition over Invocation and
`Notification`/`EventCount`; it is not a kernel object kind.

## User-level visible operations

| Op | Name | Status |
|---|---|---|
| TBD | Invoke | Operation ID and call schema remain open; no wrapper or handler is implemented. |

## Contract details still to specify

- Exact operation/argument encoding and rights for constructing an Invocation
  from management authority to the target AddressSpace and destination KeyTable,
  plus the supplied function pointer.
- Invocation derivation, rights attenuation, and caller badge semantics.
- How the caller's execution context and stack are established in the target
  AddressSpace, including nested/concurrent calls and bounded resource use.
- Fixed-width argument and return ABI, pointer/shared-memory rules, and whether
  capability transfer is supported.
- Return, fault, cancellation, and Thread-teardown outcomes.

The existing SVC's six argument words and two result words are candidates to
reconcile with the function ABI, not an approved PPC register contract.

## Implementation status

The shared `CoreType` catalogue recognizes Invocation at ID 7. There is no
userspace wrapper, kernel invocation handler, export operation, or context
migration path. A registered kind does not advertise support.
