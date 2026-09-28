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

Execution-context directions (interrupt-kernel discipline, Thread-resident
bounded invocation stack, stub-provisioned target stack) are selected in
the canonical contract; research evidence lives in `lifetime-and-authority.md`
§7. Outstanding Invocation decisions:

- What are the badge, derivation, and per-kind rights rules?
- What are the exact invocation-stack record layout, depth, and identifiers,
  and the nested/concurrent-call and bounded-resource exhaustion rules?
  (KeyKOS evidence: serializing the callee — busy targets queue callers —
  eliminates kernel-side nesting state entirely, at the cost of no reentrancy;
  Composite evidence: a bounded per-thread invocation stack allows nesting
  with call-time depth rejection. Which model, or which mix, applies to an
  Invocation target?)
- What are the validation rules for the selected return form (underflow,
  target liveness, frame skipping/double return)?
- How is a fault in a migrated frame delivered and resumed? KeyKOS/EROS
  deliver a fault as a CALL carrying a resume key to the faulting domain,
  and the keeper resumes it by invoking that key; under the selected
  kernel-internal invocation stack a migrated frame has no transferable
  continuation, so keeper-style fault delivery needs its own answer (for
  example a Thread-control authority rather than a return key). NOVA's
  alternative: the faulting EC donates itself to a keeper through a
  per-vector exception-portal capability, the keeper edits the faulting
  state through its UTCB (bounded by the portal's transfer descriptor), and
  its REPLY resumes the faulting EC.
- Whose scheduling budget does a migrated call consume, and must the
  caller's scheduling entity remain runnable while its Thread is migrated?
  (Nemesis's crosstalk argument: kernel-scheduled migrating threads destroy
  application-internal scheduling and accounting; K42's fix: the caller's
  dispatcher stays runnable while the calling thread blocks; NOVA's EC/SC
  split: the caller's scheduling context funds the donated chain.)
- How do interface return values map to the syscall result words? Are
  shared-memory pointers or capability transfer supported, and under what
  validation/authority rules?
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
