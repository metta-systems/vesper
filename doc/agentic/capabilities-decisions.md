# Capabilities decisions

This file is only a parking lot for **unanswered architectural or contract
questions**. An answered question is recorded in `capabilities-contract.md` (as a
numbered decision) and, where its reasoning is relevant, `capabilities-design.md`, and
it leaves this file. Implementation, validation, and integration work belongs in
`capabilities-implementation-plan.md`. Each question names the gate it belongs to
(the contract's [decision register](capabilities-contract.md#decision-register)).

The [Future work](#future-work) section at the end is separate: ideas we might
or might not pursue depending on how the project develops. They need no
near-term decision, do not constrain current contracts or tests, and are not
plan items.

## Protected Procedure Call (`Invocation`), CAPABILITIES-7

### Q-CAPABILITIES-1: other architectural state

What TLS, debug and other non-GPR/control state must be initialized, preserved or
isolated before protected EL0 execution? GPR/NZCV scrubbing alone is not complete
state isolation.

### Q-CAPABILITIES-2: Invocation creation through Retype

Should `AddressSpace.CreateInvocation` become Retype-style, like Thread creation (an
inert object from Retype, made usable by the authority that owns it)? To be looked
at; the maintainer leans towards switching.

### Q-CAPABILITIES-3: shared memory

Pointer and shared-memory rules, including the Rust reference contracts for shared
memory (CAPABILITIES-1).

### Q-CAPABILITIES-4: fault delivery refinements

Under CAPABILITIES-1. The mechanism and its details are selected (see the contract).
Open: a small per-AddressSpace fault queue that runs handlers one after another
instead of treating a fault during a busy handler as unhandled; resuming with edited
registers; more than one fault level per Thread.

### Q-CAPABILITIES-5: fault and lifecycle

Beyond the defined no-pop Return fault classification and the selected delivery
mechanism, what are the call lifecycle, cancellation, Thread/AddressSpace teardown,
partial-completion, and other nested/concurrent-call rules beyond the fixed depth
limit?

### Q-CAPABILITIES-6: Return to a live but unready source

If the top record's saved source AddressSpace is still live but its root/ASID is
missing or unencodable, is that a third Return fault, a `ReturnTargetRetired` fault,
or an ordinary error? It currently surfaces as the preparation error without a pop.

### Q-CAPABILITIES-7: freezing the provisional PPC convention

Under CAPABILITIES-9. The maintainer decides when it becomes frozen ABI. It works
end-to-end through real Call/Return; no validation or plan item gates the freeze, and
work should not be planned around it. "Provisional", "experimental" or "until
field-tested" in the documents means this maintainer decision.
- the common native target-body return convention (`extern "C"` body returning a
  `#[repr(C)]` two-`u64` struct in x0/x1).

## Other unanswered decisions

### Q-CAPABILITIES-8: shared-address conflicts and per-target protection

Under CAPABILITIES-1. Machine-local namespace and address-reservation conflicts for
shared addresses (fbufs, common mappings), and which protection features each target
backend must provide; resource-accounting and bounded-work mechanisms that hold even
when a caller bypasses its libOS.

### Q-CAPABILITIES-9: managers, completion and ordering

Under CAPABILITIES-2. The bookkeeping, failure and coordination protocol between the
managers of a libOS composition (multiple or hierarchical KeyMasters), and the
completion handshake for retirement followed by background cleanup;
invocation-versus-retirement ordering (after which point new operations and
derivations cannot proceed) and how revoke completion is observed; the checked
management-operation schemas.

### Q-CAPABILITIES-10: selective invalidation

Under CAPABILITIES-2 and CAPABILITIES-6. Selective invalidation of a branch of derived
capabilities while the shared object remains live, including completion and safe-reuse
rules.

### Q-CAPABILITIES-11: rights, attenuation and badges

Under CAPABILITIES-4. Per-kind semantic rights and the operation-to-rights matrix
before bit assignments freeze; permitted attenuation of rights, extents, badges and
budgets; badge width, zero-badge meaning, and mint/rebadge policy; Call-only
Invocation distribution.

### Q-CAPABILITIES-12: bootstrap and well-known slots

Under CAPABILITIES-4. Ownership of well-known `KeySlot` constants: shared object API
versus bootstrap/libOS composition layout; bootstrap Threads, AddressSpaces,
capacities, grants and handoff records.

### Q-CAPABILITIES-13: ABI version discovery

Under CAPABILITIES-9. ABI version and optional-operation discovery for separately built
components: how a component learns the running kernel's ABI revision and supported
operations, and what happens on a mismatch. Not needed while everything is rebuilt
together from one tree.

### Q-CAPABILITIES-14: keys and storage, the open parts

Under CAPABILITIES-3. Thread lifecycle integration; in-place authority mutation;
unsafe full-borrow and fbuf protocols; per-kind details.

### Q-CAPABILITIES-15: memory and ASIDs, the open parts

Under CAPABILITIES-6. Map, remap, revoke and no-reuse schemas; sharing-mode and fbuf
protocols; sanitization beyond Retype-carved Frames; ASID reuse and multi-pool
partitioning; device rules. The memory area's own open questions are in
[`memory-decisions.md`](memory-decisions.md).

### Q-CAPABILITIES-16: status codes still to define

Under CAPABILITIES-9. The status for a missing or mismatched self-table capability;
wait-operation errors.

## Future work

Not decisions to make now and not planned work; revisit only if the project
needs them.

- **N-CAPABILITIES-1: extended IPC outputs (`x1..x7`) and a per-thread IPC buffer.**
  Returning more than two result words from a capability invocation, with message
  spill to a per-thread buffer. Today every ordinary invocation writes only `x0..x2`
  and preserves everything else (CAPABILITIES-10); adopting extended outputs would be
  a contract revision of that rule.
- **N-CAPABILITIES-2: temporal-VA protection for stale raw pointers.** Kernel
  guarantees that an old raw pointer cannot reach replacement memory after its virtual
  address is legitimately reused — address quarantine, keeping revoked addresses
  inaccessible for a surviving Thread's lifetime, or stronger stale-pointer detection.
  Today the kernel gives no such guarantee and outside mechanisms own the prevention
  (contract, [protection requirements and
  boundaries](capabilities-contract.md#protection-requirements-and-boundaries)). Would
  need its own separately scoped design.
- **N-CAPABILITIES-3: 64-bit allocation generations.** Explore whether widening kernel
  allocation generations (`ObjectId`, pool slot metadata and pending-record
  generations) from 32 to 64 bits is feasible and worthwhile. A slot exhausting its
  generation is retired from reuse today; 64 bits would make that unreachable in
  practice, at the cost of larger identities in every capability entry, pool metadata
  record and pending record. Would need a measured look at those layouts and the
  `KeyEntry` payload size.
- **N-CAPABILITIES-4: continuing carved-object generations across re-carving.**
  Instead of a fresh header starting at the first generation, investigate whether
  re-carving could read the previous header's generation and continue from it. It
  would need the old generation to survive reclamation (the bytes are not sanitized,
  or the generation is kept elsewhere), a rule for memory re-carved at a different
  offset or as a different kind, and a check that it adds protection beyond the
  retire-time bump and completed cleanup that the contract already relies on
  (CAPABILITIES-19).
- **N-CAPABILITIES-5: count ranges on CopyDerive and Move.** An optional count that
  copies or moves a run of consecutive slots in one operation, like NOVA's `ctrl_pd`
  selector order. The framebuffer hand-off needs only two to four 2 MiB Frames, so
  single-slot CopyDerive is enough today; ranges would matter for hand-offs of
  hundreds of small Frames.
- **N-CAPABILITIES-6: keys in call results.** Letting `Thread.Return` carry a key back
  to the caller, and letting `CallWithKey` lend more than one key. Today a call lends
  one key and results carry data only.
- **N-CAPABILITIES-7: releasing the stacks of a faulted Thread.** A Thread that faults
  inside a pooled call keeps its stack until its continuations are dropped. The
  resource manager is meant to clean up Threads stuck in the faulted state, which
  would also free those stacks; how it unwinds or retires them is not designed (see
  [CAPABILITIES-25](capabilities-contract.md#capabilities-25-stack-pools)).
- **N-CAPABILITIES-8: chainboot with cached RAM.** The `mmu` build of chainboot maps
  RAM as Normal non-cacheable with the data cache off, so atomic read-modify-write
  still does not complete on the Raspberry Pi's cores (exclusives need a global
  monitor the Pi does not have for uncached memory) and `build-chainboot` fails on any
  such instruction. Mapping RAM write-back cacheable with the data cache on would make
  atomics work, at the price of cache maintenance: clean the kernel image and
  invalidate the instruction cache before the jump; clean and invalidate the whole
  data cache by set/way before the MMU goes off, so that stale dirty lines cannot
  overwrite the kernel later; clean the mailbox buffer before each send and invalidate
  it after the answer, or keep the buffers in a non-cacheable region. Worth doing only
  if chainboot needs atomics; QEMU does not model caches, so the board is the test.
