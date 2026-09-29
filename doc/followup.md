# Pending architectural decisions

This file is only a parking lot for **unanswered architectural or contract
questions**. Settled decisions belong in `doc/nucleus_capabilities.md` and, where
lifetime/authority analysis is relevant, `doc/lifetime-and-authority.md`.
Implementation, validation, and integration work belongs in
`doc/capabilities_implementation_plan.md`. Remove an item here once it is
resolved and recorded in its architectural document.

## D5: scheduler-shared thread records

Unanswered questions before implementing the scheduler record ABI:

- What is the exact `Scheduler.ShareRegion` operation ID, register schema,
  authority/rights, result, and failure contract?
- How does a scheduler declare its fixed-stride record table: descriptor,
  capacity, page-to-slot mapping, and record-slot allocation?
- What are the exact shared-record fields and kernel-versus-scheduler
  transition rules, publication/snapshot protocol, event-summary indexing,
  and record retirement/reuse behavior?
- What are the Thread creation and donation schemas, and how is strict-tree
  scheduler ownership enforced?
- What precisely does a `Brand` name, how is its handler bound, and how does it
  relate to `IRQHandler`?


## D7: Protected Procedure Call (`Invocation`)

Selected execution-context, stack-extent and `InvalidStack` error-family
contracts, reason/value catalogue, reason IDs and status 32 are in the canonical
contract, together with stack-predicate and admission-stage order.
Research evidence and comparisons are in
`doc/lifetime-and-authority.md` §7.

### Remaining PPC architectural decisions

- **Target execution status:** which non-NZCV target-entry SPSR controls
  (execution mode, interrupt masks and supported control bits) should Call
  install for the trusted EL1t fixture and eventual EL0 components? What is
  target-owned versus Thread-owned, without blindly inheriting source control
  bits or zeroing the whole SPSR?
- **Other architectural state:** what TLS, debug and other non-GPR/control
  state must be initialized, preserved or isolated before protected EL0
  execution? GPR/NZCV scrubbing alone is not complete state isolation.
- **Authority:** Invocation derivation/CopyDerive restrictions, rights
  attenuation, and source badges.
- **Shared memory and transfer:** pointer/shared-memory rules and whether
  optional capability transfer is supported.
- **Fault and lifecycle:** fault delivery/resumption for a migrated frame;
  call/return behavior when an AddressSpace or Thread is retired; cancellation,
  teardown, partial completion, and other nested/concurrent-call constraints
  beyond the fixed depth limit.
- **Scheduling attribution:** the Call-to-Return interval is stamped and is
  currently attributed to the source Thread's own DCB; how hierarchical
  schedulers should observe and attribute that work remains deferred.

## Other unanswered decisions

- **D2/D6:** selective invalidation of a branch of derived capabilities while
  the shared object remains live, including completion and safe-reuse rules.
- **D4/bootstrap:** ownership of well-known `KeySlot` constants—shared object
  API versus bootstrap/libOS composition layout.
- **D6:** whether ASID identity should occupy high `VirtAddr` bits, or whether
  that proposal should be rejected.
- **D8:** Time operation schemas and conservation rules: Donate consumed-self
  semantics, Split destination-slot behavior, Merge/Query, and unused-budget
  return on deletion/drop.
