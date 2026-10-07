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


### Remaining PPC architectural decisions


- **Other architectural state:** what TLS, debug and other non-GPR/control
  state must be initialized, preserved or isolated before protected EL0
  execution? GPR/NZCV scrubbing alone is not complete state isolation.
- **Call-only Invocation distribution (D4):** what are the permitted
  derivation/CopyDerive restrictions, rights attenuation, and source badges?
- **Shared memory and transfer:** pointer/shared-memory rules and whether
  optional capability transfer is supported.
- **Fault delivery refinements (D1):** the mechanism and its details are
  selected (see the contract). Open: a small per-AddressSpace fault queue that
  runs handlers one after another instead of treating a fault during a busy
  handler as unhandled; resuming with edited registers; more than one fault
  level per Thread.
- **Fault and lifecycle:** beyond the defined no-pop Return fault
  classification and the selected delivery mechanism, what are the call lifecycle,
  cancellation, Thread/AddressSpace teardown, partial-completion, and other
  nested/concurrent-call rules beyond the fixed depth limit?
- **Return-key handoff to component init:** the AddressSpace builder installs
  the Slot(1) sentinel during provisioning and passes its deterministic key to
  the component's init, which records it for the export adapter. Through what
  channel does init receive it (entry register, init argument block, DCB
  field), and is it part of the general init-handoff record?
- **Return to a live but unready source:** if the top record's saved source
  AddressSpace is still live but its root/ASID is missing or unencodable,
  is that a third Return fault, a `ReturnTargetRetired` fault, or an ordinary
  error? It currently surfaces as the preparation error without a pop.
- **Scheduling attribution:** how should hierarchical schedulers observe and
  attribute migrated work beyond the source Thread's own DCB accounting?
- **Freezing the provisional PPC conventions (D9):** the maintainer decides
  when each becomes frozen ABI. Both work end-to-end through real Call/Return;
  no validation or plan item gates either freeze, and work should not be
  planned around them. "Provisional", "experimental" or "until field-tested"
  in the documents means this maintainer decision.
  - the `x9` Call-time target-SP register;
  - the common native target-body return convention (`extern "C"` body
    returning a `#[repr(C)]` two-`u64` struct in x0/x1).

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
