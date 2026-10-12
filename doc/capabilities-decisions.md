# Capabilities decisions

This file is only a parking lot for **unanswered architectural or contract
questions**. Settled decisions belong in `doc/capabilities-contract.md` and, where
their reasoning is relevant, `doc/capabilities-design.md`.
Implementation, validation, and integration work belongs in
`doc/capabilities-implementation-plan.md`. Remove an item here once it is
resolved and recorded in its architectural document.

The [Future work](#future-work) section at the end is separate: ideas we might
or might not pursue depending on how the project develops. They need no
near-term decision, do not constrain current contracts or tests, and are not
plan items.

## D5: scheduler-shared thread records

Unanswered questions before implementing the scheduler record ABI:

- What is the exact `Scheduler.ShareRegion` operation ID, register schema,
  authority/rights, result, and failure contract?
- How does a scheduler declare its fixed-stride record table: descriptor,
  capacity, page-to-slot mapping, and record-slot allocation?
- What are the exact shared-record fields and kernel-versus-scheduler
  transition rules, publication/snapshot protocol, event-summary indexing,
  and record retirement/reuse behavior?
- What is the scheduler donation schema, and how is strict-tree
  scheduler ownership enforced?
- Where does the dispatch operation (Thread, Time, priority, timeout) live
  (Scheduler, Thread or Time), and how are block and expiry events posted to
  the scheduler Thread through its records?


## D7: Protected Procedure Call (`Invocation`)


### Remaining PPC architectural decisions


- **Other architectural state:** what TLS, debug and other non-GPR/control
  state must be initialized, preserved or isolated before protected EL0
  execution? GPR/NZCV scrubbing alone is not complete state isolation.
- **Invocation creation through Retype:** should `AddressSpace.CreateInvocation`
  become Retype-style, like Thread creation (an inert object from Retype, made
  usable by the authority that owns it)? To be looked at; the maintainer
  leans towards switching.
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
- **Return to a live but unready source:** if the top record's saved source
  AddressSpace is still live but its root/ASID is missing or unencodable,
  is that a third Return fault, a `ReturnTargetRetired` fault, or an ordinary
  error? It currently surfaces as the preparation error without a pop.
- **Scheduling attribution:** how should hierarchical schedulers observe and
  attribute migrated work beyond the source Thread's own DCB accounting?
- **Component outcome through the Call status:** a successful `Thread.Return`
  always reaches the caller as `x0 = 0` with `r0`/`r1` in `x1`/`x2`, so
  Meddle-generated interfaces spend `r0` on their own outcome word (success,
  exception index, malformed arguments) and get one result word. Should
  `Thread.Return` carry a component outcome into the caller's `x0` instead,
  for example in a status range or bit the kernel never uses for its own
  status codes, leaving both words for results? Affects the Return and Call
  register contracts (D7/D9), `decode_syscall_result`, the export adapter and
  the Meddle call convention in `libs/meddle-runtime`.
- **Freezing the provisional PPC conventions (D9):** the maintainer decides
  when each becomes frozen ABI. Both work end-to-end through real Call/Return;
  no validation or plan item gates either freeze, and work should not be
  planned around them. "Provisional", "experimental" or "until field-tested"
  in the documents means this maintainer decision.
  - the `x9` Call-time target-SP register;
  - the common native target-body return convention (`extern "C"` body
    returning a `#[repr(C)]` two-`u64` struct in x0/x1).

## Other unanswered decisions

- **D1:** machine-local namespace and address-reservation conflicts for shared
  addresses (fbufs, common mappings), and which protection features each
  target backend must provide; resource-accounting and bounded-work mechanisms
  that hold even when a caller bypasses its libOS.
- **D2:** the bookkeeping, failure and coordination protocol between the
  managers of a libOS composition (multiple or hierarchical KeyMasters), and
  the completion handshake for retirement followed by background cleanup;
  invocation-versus-retirement ordering (after which point new operations and
  derivations cannot proceed) and how revoke completion is observed.
- **D2/D6:** selective invalidation of a branch of derived capabilities while
  the shared object remains live, including completion and safe-reuse rules.
- **D4:** per-kind semantic rights and the operation-to-rights matrix before
  bit assignments freeze; permitted attenuation of rights, extents, badges and
  budgets; badge width, zero-badge meaning, and mint/rebadge policy.
- **D4/bootstrap:** ownership of well-known `KeySlot` constants—shared object
  API versus bootstrap/libOS composition layout.
- **D6:** whether ASID identity should occupy high `VirtAddr` bits, or whether
  that proposal should be rejected.
- **D8:** whether the root budget is infinite or a large finite amount (both
  are buildable through the Kickstart `infinite-boot-time` feature) and who
  replenishes a finite one; what happens to remaining budget when a Time is
  deleted; dispatch register schema and protection against races between
  user-level dispatch and kernel events (Composite's scheduling token); clock
  source and rounding of charged time.
- **IRQ follow-up:** whether a Thread may bind one Notification for both
  PPC-server and interrupt waits (seL4 `TCB_BindNotification`).
- **D9:** ABI version and optional-operation discovery for separately built
  components: how a component learns the running kernel's ABI revision and
  supported operations, and what happens on a mismatch. Not needed while
  everything is rebuilt together from one tree.
- **D1/D6/D9 — DebugConsole byte transport (parked):** how a generally
  available console receives its bytes. (A) keep the pointer-based Write: the
  nucleus needs checked user-memory reads (caller-AddressSpace authorization,
  input snapshot, copy-fault recovery), which no other operation currently
  needs. (B) a register-inline operation, e.g. a length plus up to 40 bytes in
  `x3..x7` with client chunking, so the nucleus never reads user memory. Until
  this is decided the console stays a debug-only prototype.

## Future work

Not decisions to make now and not planned work; revisit only if the project
needs them.

- **Extended IPC outputs (`x1..x7`) and a per-thread IPC buffer:** returning
  more than two result words from a capability invocation, with message
  spill to a per-thread buffer. Today every ordinary invocation writes only
  `x0..x2` and preserves everything else (contract, ordinary control
  invocation baseline); adopting extended outputs would be a contract revision
  of that rule.
- **Temporal-VA protection for stale raw pointers:** kernel guarantees that an
  old raw pointer cannot reach replacement memory after its virtual address is
  legitimately reused — address quarantine, keeping revoked addresses
  inaccessible for a surviving Thread's lifetime, or stronger stale-pointer
  detection. Today the kernel gives no such guarantee and outside mechanisms
  own the prevention (contract, [protection requirements and
  boundaries](capabilities-contract.md#protection-requirements-and-boundaries)).
  Would need its own separately scoped design.
- **Six-word PPC result payload:** widening the PPC result from two to six
  `u64` words if component interfaces need more register-only results. Would
  need a coordinated change to the Return transport, wrappers and decoders.
- **64-bit allocation generations:** explore whether widening kernel
  allocation generations (`ObjectId`, pool slot metadata and pending-record
  generations) from 32 to 64 bits is feasible and worthwhile. A slot
  exhausting its generation is retired from reuse today; 64 bits would make
  that unreachable in practice, at the cost of larger identities in every
  capability entry, pool metadata record and pending record. Would need a
  measured look at those layouts and the `KeyEntry` payload size.
- **Continuing carved-object generations across re-carving:** instead of a
  fresh header starting at the first generation, investigate whether
  re-carving could read the previous header's generation and continue from
  it. It would need the old generation to survive reclamation (the bytes
  are not sanitized, or the generation is kept elsewhere), a rule for memory
  re-carved at a different offset or as a different kind, and a check that it
  adds protection beyond the retire-time bump and completed cleanup that the
  contract already relies on.
- **Budget from a scheduler's own bound Time:** a Time bound to a live Thread
  can't give budget away, so a scheduler gives its children budget only from
  Times not bound to a Thread (contract, bound Times). Revisit whether this
  can be relaxed or reworked, for example letting the bound Thread itself (or
  its scheduler) transfer out while still refusing other key holders, without
  reopening the risk of an interrupt Thread's Time being drained.
- **Kernel-published identity in `TPIDRRO_EL0`:** using the EL0 read-only,
  EL1-writable register to publish per-Thread identity to userspace, as Linux
  does for per-CPU data. Today it is always zero.
- **Driver probe/init calls:** Kickstart hands a driver everything through its init page and never calls into it. A device manager may need a probe/init part of the driver interface (asking a driver whether it can drive a device, or telling it to start or stop); look into what that would be and how it is called.
