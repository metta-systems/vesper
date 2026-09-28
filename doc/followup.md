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

Outstanding Invocation decisions:

- What are the badge, derivation, and per-kind rights rules?
- How are the caller's execution context and stack established during
  migration, including nested or concurrent invocations and bounded resource
  exhaustion?
- What fixed-width argument/result ABI is used? Are shared-memory pointers or
  capability transfer supported, and under what validation/authority rules?
- What are the call lifecycle, return, fault, cancellation, timeout, and
  Thread/AddressSpace teardown semantics?

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
