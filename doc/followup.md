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
bounded invocation stack, stub-provisioned target stack), the return-operation
form, the return key's entry representation/construction/presence, and the
return-validation classification are selected in the canonical contract;
research evidence lives in `lifetime-and-authority.md` §7. Outstanding
Invocation decisions:

- What are the badge, derivation, and per-kind rights rules?
- What are the exact invocation-stack record layout, depth, and identifiers,
  and the nested/concurrent-call and bounded-resource exhaustion rules?
  (Expanded with options below: *PPC invocation-stack design — decisions
  pending*. Nesting policy assumed: Composite-style nesting with a call-time
  depth bound, not KeyKOS-style callee serialization — say so if that should
  be revisited.)
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
- What are the call lifecycle, cancellation, timeout, and
  Thread/AddressSpace teardown semantics? (The `Return` mechanism and its
  protocol-failure classification are selected: a `Return` that cannot
  complete its protocol is a fault to the Thread's fault handler — "illegal
  return" for depth-zero underflow, "return target retired" for a dead
  target AddressSpace — with no pop; on a Call-only Invocation it is a
  recoverable `InvalidOperation`.)

### PPC invocation-stack design — decisions pending (2026-09-28)

Next round's work: pick the lettered options below, record the result in
`doc/nucleus_capabilities.md` (communication section + D7) and
`doc/capabilities_implementation_plan.md`, then implement. The evidence lives
in `doc/lifetime-and-authority.md` §7 (Fluke, Composite sinv/sret, K42/Tornado,
KeyKOS/EROS, NOVA, seL4 MCS, Opal, Nemesis, Mungi PDX, Clouds/Grasshopper,
Multics/VMS); the contract selected so far is in the contract's "Selected PPC
execution-context directions". No need to re-read either unless a detail below
is unclear.

**Fixed context (do not re-derive).**
- Maintainer constraints: the kernel allocates no runtime memory — the libOS /
  root scheduler supplies every stack and invstk region (threads are created
  only by the root scheduler); hierarchical userspace schedulers with TimeCap
  donation; an AS transition during `Call` stamps the caller's used time and
  marks the record "callee on the caller's behalf".
- Two findings that shape the design. (1) Charging is free in all three
  references — the payer follows the thread (Composite's `sinv_call` never
  touches tcap state; seL4 donates the SC; NOVA never hands the SC over) — so
  "A's used time" is the thread's own DCB counter. The novel part is the
  per-call stamp (8 B/frame), which no reference has; the "on behalf" marker
  is the record itself. Charging discipline (seL4's
  charge-before-switch-never-roll-back) lands with Time/D8, still an excluded
  sketch (`current_time_ns() == 0`). (2) Migration must switch the cspace as
  well as the translation context, and all three references dodge that because
  their callee is a separate thread/EC with its own CSpace roots — vesper's
  record must carry the caller's cspace, and `Thread.keytable_addr` is a raw
  kernel-window `u64`, which sits badly with D3 once copied per frame.
- Current state (verified in-repo): Thread entry is 40 B in a boot-carved pool
  (`nucleus.rs:403` asserts `carve_size(3) <= 256` — an inline invstk blows
  this up ~20×); no per-CPU state anywhere (`current_thread` is one global
  `Option<u32>`), so Composite's per-core cached `invstk_top` is SMP-later;
  everything runs at EL1 (`origin_el` is future-proofing); no Thread-create op
  exists; DCB pages are never added in production and `activate_thread` has
  no callers; the wait path stores a kernel-stack frame address in
  `ExecutionContext::Parked`.

**Record sketch (48 B with stamp, 40 B without):** caller-AS `ObjectId`,
caller cspace, return PC (ELR_EL1 — resume after the `Call` SVC), user SP,
optional `stamp_ns`, one `origin_el` byte. No callee-AS field (it is
`Thread.address_space` while migrated); no callee-clobbered registers
(x2–x17 are caller-saved; x0/x1 return values come from the callee's live
registers at the `Return` SVC); no TLS field until TLS is per-domain.

**Decisions — pick one per letter (recs are mine):**
- **A. Record storage.** A1 inline `[Continuation; D]` in the Thread entry
  (Composite's shape, cheapest per op, depth becomes a compile-time constant,
  every Thread construction site and the 256 B fixture grow ~20×). A2 a
  separate `ObjectPool<Continuation>` region per Thread with
  `invstk: ObjectId` in the Thread, push/pop resolved through `Access` —
  libOS sizes the pool via `PoolCapacities`, D3-clean identity, small Thread
  entry; costs one extra pool and one resolve per push/pop. **Rec A2.**
- **B. Depth and exhaustion.** Fixed D (Composite 16; 8 is likely deeper than
  real nesting). Push-full → defined error to the caller as a new D9 status
  (`CapError::NestingDepth`), not a raw errno, not `PoolExhausted`. Depth
  lives in the shared catalogue. Underflow is already a fault (selected).
- **C. Cspace representation (prerequisite-ish).** C1 raw kernel address in
  the record now (status-quo shape, explicit interim debt). C2 pool-back
  KeyTables first (D5 KeyMaster direction) so the record carries `ObjectId`s.
  **Rec C2** if the slice budget allows, else C1 recorded as interim.
- **D. Callee cspace at `Call`.** The Invocation payload must carry it. D1
  composition convention — the `x3` destination KeyTable of
  `CreateInvocation` is the callee's table, stored in the payload, no schema
  change. D2 a new explicit operand. D3 long-term: AddressSpace gains a
  "self table" identity. **Rec D1 now, D3 eventually.**
- **E. Time.** E1 thread-scoped charging only (per-call visible only as DCB
  deltas) vs E2 E1 plus an 8 B `stamp_ns` per frame, stamped at `Call`/`Return`.
  **Rec E2** (maintainer constraint); the arithmetic is inert until Time.
- **F. SP validation at `Call`.** F1 range + 16 B alignment only (NOVA-style).
  F2 full mapping walk at admission. F3 none (Composite/seL4 trust) — but an
  unvalidated SP currently faults with no defined landing spot (migrated-frame
  fault delivery is open). **Rec F1 now, F2 when fault delivery exists.**
- **G. A migrated thread that blocks in a wait.** G1 invstk becomes an
  orthogonal Thread field; `ExecutionContext` keeps describing what the thread
  is doing, and waits still park on the kernel stack (the contract only says
  the parked-frame path is not the PPC mechanism). G2 fold blocking into the
  invstk (fully interrupt-kernel, no kernel-stack frames anywhere) — the
  complete selected direction, but it rewrites `park_and_switch`, the
  pending-record terminal transitions, and the resume path. **Rec G1 for this
  slice, G2 as a follow-up.**

**Defaults to take unless told otherwise:** `invstk_top` lives in the Thread
(no per-CPU cache yet); `origin_el` and `stamp_ns` are the only
future-proofing fields.

**Slice sequence once the letters are picked:** shared record/depth constants
with host tests → region pool plus `invstk_push`/`pop` in the nucleus →
`Invocation.Call`/`Return` dispatch with the context switch, extending the
boot fixture's existing `create_invocation` test into a real nested
call/return.

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
