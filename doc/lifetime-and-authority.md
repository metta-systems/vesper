# Lifetime and authority semantics

## Status and relationship to the capability contract

Design investigation recorded on 2026-09-05. **This document records analysis, alternatives, recommendations, and unanswered questions; it does not approve a new ABI or lifecycle model.** Recording the discussion is not approval of the proposed defaults.

- [Nucleus capabilities](nucleus_capabilities.md) remains the canonical contract and decision register (D1–D9).
- [Capability implementation plan](capabilities_implementation_plan.md) remains the dependency-ordered implementation checklist. The tasks here expand the lifecycle/authority work; they do not replace its phase ordering or mark existing tasks complete.
- **Current code** describes the inspected implementation, not a desired contract. Recheck reachability and source details before implementing a slice.
- **Recommendation** identifies a proposed direction, not an accepted decision.
- **OPEN / DECISION REQUIRED** identifies an unanswered question that must be settled before dependent implementation.
- All `- [ ]` items are outstanding decision, implementation, or validation work. Implementation tasks are conditional on the relevant decisions; alternative designs are not instructions to implement every option.

When a decision is approved, record its chosen semantics, rationale, and compatibility impact in the canonical contract first, then reconcile this document and the implementation plan before changing dependent code.

## Working hypothesis: thin handles, authoritative kernel checks

The motivating position is that userspace representations should not closely track kernel object lifecycle: after an object is disposed, invoking its userspace capability should return an error.

**Recommendation:** preserve that direction for ordinary fallible invocations, with an important qualification:

> Userspace handles need not mirror liveness, provided the kernel validates the intended identity and authority on each invocation. Direct memory access and ownership-consuming operations need additional contracts.

Not tracking whether an object is alive is different from not knowing which incarnation of an object or slot a handle names. A userspace liveness query is at most an observation; it cannot replace validation at the operation's authorization/commit boundary.

The desired separation is:

- A **handle** names a local entry or its incarnation.
- A **capability entry** carries authority over a resource or delegation scope.
- A **resource** has independently managed identity, state, and physical lifetime.
- An **in-flight operation or mapping** can retain dependencies after its initiating handle disappears.

Phantom types improve ergonomics, not authority or lifetime enforcement. Copying a Rust handle is not kernel capability derivation.

## 1. Stale handles and identity reuse

### Current code and counterexample

[`Key<T>`](../libs/object/src/key.rs) contains only `KeySlot` and `PhantomData<T>`. The ordinary transport supplies a slot, operation, and arguments, not the identity of the object previously occupying that slot.

1. Slot 12 contains endpoint A.
2. Userspace retains a key naming slot 12.
3. A's entry is deleted and the slot is reused for endpoint B.
4. The retained key invokes slot 12 and can operate on B.

Even a type check cannot distinguish two endpoints. This need not escalate authority across domains—B is already in the caller's table—but it can use replacement authority unintentionally. Thus stale invocation does not necessarily fail under the current slot-only representation.

### Alternatives

| Choice | Consequences | Complexity |
|---|---|---|
| Slots name their current occupant, like file descriptors | Accept reuse hazards; userspace coordinates slot ownership | Low kernel complexity |
| Exclusive userspace slot allocator with owned slots and borrowed handles | Prevent accidental reuse while handles exist, provided every mutation follows that allocator | Medium; requires enforceable runtime discipline |
| Kernel-checked slot incarnation supplied with invocation | Retained handles fail after replacement without monitoring liveness | Medium cross-layer change, including ABI/bootstrap |

**Recommendation:** use incarnation-bearing handles if “stale invocation must error” is a promised property. A slot-only interface is defensible, but promises only to resolve the current occupant. A userspace generation precheck alone leaves a check/use race.

Three distinct identity problems must be covered:

- **Slot incarnation:** whether a local handle refers to a replaced entry.
- **Object/domain incarnation:** whether a kernel reference refers to recycled storage.
- **Delegation-scope identity:** whether the authority branch has been revoked.

These may share machinery, but one generation counter does not automatically solve all three. A generation is not a secret or a substitute for capability authorization.

**OPEN / DECISION REQUIRED — D3/D9:**

- [ ] Choose current-occupant versus incarnation semantics for userspace handles.
- [ ] Define identity scope, including table/domain context and how cross-domain transfer produces a receiver-local handle.
- [ ] Define object/domain allocation identities and their authoritative validation metadata.
- [ ] Define generation widths, wrap/exhaustion policy, and pool/table reuse rules; silent wrap must not resurrect stale identities.
- [ ] If the wire representation changes, specify coordinated migration, bootstrap encoding, and result/error schemas.

## 2. Delete, revoke, retire, and reclaim are different operations

The canonical contract already separates entry deletion from revocation and safe reuse. “Disposed” needs a more precise operation-specific meaning.

| Term | Semantic distinction to preserve |
|---|---|
| Delete a capability | Remove this table entry; object-specific rules determine any additional cleanup |
| Revoke a delegation scope | Prevent further use of the specified authority and its covered descendants |
| Retire an object | Stop admitting new operations and resolve existing uses according to its contract |
| Reclaim storage/backing | Reuse only after retained operations, mappings, hardware access, and other dependencies are retired |

**Proposed internal resource lifecycle:**

```text
Live → Retiring → Quiescent → Reclaimed
```

Userspace need not duplicate this state machine. The kernel and any trusted resource manager need enforceable transitions. Authority retirement may precede physical reclamation, and revoking one delegation branch need not retire the shared object for other branches.

### Explicit retirement authority

**Recommendation:** ordinary capabilities authorize use; distinct owner/manager authority authorizes resource retirement. Ordinary deletion should not implicitly destroy sibling holders' authority.

Benefits: predictable delegation, clearer resource policy, and no universal expensive destructor hidden behind local handle deletion.

Cost: an orphan policy is necessary when the manager dies. Parent supervision, domain teardown, or allocation-scope reclamation must eventually recover abandoned resources. Merely moving lifetime policy to userspace does not solve manager failure.

### Last-capability deletion initiates retirement

This is an alternative for particular object kinds, not a recommended universal rule. Reference counting capabilities alone does not account for pending invocations, mappings, active donations, internal relationships, or cycles. Final deletion may initiate substantial asynchronous work rather than immediately free storage.

A pin that keeps storage safe is **not** permission to continue using revoked authority. Physical lifetime and logical authorization must remain separate.

**OPEN / DECISION REQUIRED — D2/D3/D7/D8:**

- [ ] Define per-kind deletion effects and whether final-capability deletion initiates retirement.
- [ ] Define who holds resource-retirement authority and its relationship to allocation/delegation authority.
- [ ] Define manager death, orphan recovery, and supervisor responsibilities without introducing ambient authority.
- [ ] Define invocation-versus-retirement ordering: admission, commit, already-committed effects, and the point after which new operations/derivations cannot proceed.
- [ ] Define revoke completion: immediate invalidation versus full quiescence, bounded/incremental work, and how completion is observed.
- [ ] Define outcomes for operations already pending when their capability, scope, resource, or domain is retired. Local slot deletion need not mean cancellation; the choice must be explicit.

## 3. Guarded kernel access and storage lifetime

### Current code

- [`KeyEntry`](../kernel/nucleus/src/api/key_entry.rs) has a pointer-payload generation field, but constructors initialize it to zero and object access checks type rather than allocation generation/liveness.
- [`ObjectRef`](../kernel/nucleus/src/objects/object_ref.rs) accepts `&T`, erases its lifetime, is copyable, and later manufactures shared or mutable references based on a type tag. `T: 'static` does not mean the particular instance lives forever. Separate aliases can also manufacture incompatible mutable references.
- [`ObjectPool`](../kernel/nucleus/src/objects/object_pool.rs) tracks allocation bits and indexes, not incarnations or retirement.
- [`KeyTable`](../kernel/nucleus/src/objects/key_table.rs) permits null insertion to increment occupancy; unrestricted mutable entry access can bypass membership bookkeeping.
- The nucleus entry path uses [`IRQSafeNullLock`](../libs/locking/src/lib.rs), which masks interrupts rather than providing general multicore exclusion.

Returning an error requires safe validation **before** dereferencing an object. Comparing against metadata in already-freed backing is not a valid generation check.

### Proposed foundation

Build on typed pools rather than introducing general heap ownership:

1. Persistent capabilities carry checked object identities, conceptually `(pool, index, generation)`.
2. Authoritative allocation metadata records allocation, generation, and retirement state, with its own valid backing lifetime.
3. An owning access context resolves identities and provides short-lived guarded access.
4. Multi-object operations resolve same-table/same-object aliases explicitly.
5. Object references and incompatible guards end before scheduling or context switching.
6. Pending operations retain checked identities and explicit reservations, not borrowed Rust references.

**OPEN / DECISION REQUIRED — D3, with D1 for protection bindings:** choose serialization/locking, reference/pin accounting, authoritative metadata placement, and safe pool-backing lifetime. Initially serialized single-core transitions can reduce complexity; this is not an SMP guarantee.

- [ ] Establish pool backing, alignment, capacity, zero-sized-type policy, and charged metadata requirements.
- [ ] Replace lifetime-erasing constructors and unrestricted object casts with access through the approved owning/locking context.
- [ ] Implement allocated/incarnation/retirement checks before object dereference.
- [ ] Enforce unique kernel type mappings and handle aliased operands without fabricating exclusivity.
- [ ] Make table insertion/removal/mutation preserve occupancy and identity invariants.
- [ ] Ensure scheduling occurs after guards end; add real cross-core synchronization before permitting concurrent access.

## 4. Authority, delegation, badges, and revocation scopes

### Per-operation authorization

[`Rights`](../libs/object/src/rights.rs) aliases MAP, WRITE, and SEND to the same bit. Reusing bits across kinds is permissible; within a frame's semantics, permission to map must not accidentally imply write access.

[`KeyTableKey::grant_to`](../libs/object/src/key_table.rs) currently requests `Rights::all()`. This is request encoding, not an approved authority-amplification rule. The eventual kernel handler must enforce the source authority ceiling.

| Operation | Authority that must be established |
|---|---|
| Derive into another table | Source delegation permission, permitted attenuation, destination installation authority |
| Map a frame | Backing access, target protection-context authority, requested permission/attribute ceiling |
| Retire a resource | Explicit resource-management authority, not mere use permission |
| Revoke descendants | Management authority over that delegation scope |
| Transfer through IPC | Transfer permission and an authorized receiver destination |
| Activate a domain | Domain-control authority and valid execution budget |

**Recommendations:**

- Full table management is stronger than accepting a capability into a reserved receive slot. Keep destination authority explicit; narrower installation windows are a possible later refinement.
- Badges are authority metadata, not arbitrary caller identity. Define who may mint/rebadge a sender view; ordinary delegation must not silently allow impersonating another authority-bearing view.
- Copy is per-kind: Untyped cannot duplicate independent allocation watermarks; Reply cannot create independently usable replies; Time must conserve budget. A proposed Frame copy starts unmapped, while a move preserves its mapping binding.
- Validate and reserve a destination before removing source authority. Resolve aliases before constructing references or mutating operands.

**OPEN / DECISION REQUIRED — D4, plus D2/D3/D6/D8 where applicable:**

- [ ] Define per-kind semantic rights and the operation-to-rights matrix before freezing bit assignments.
- [ ] Define permitted attenuation of rights, extents, badges, budgets, and other per-kind authority.
- [ ] Set badge width, zero-badge meaning, and mint/rebadge/notification-bit policies.
- [ ] Define destination table management versus installation/receive authority.
- [ ] Finalize bootstrap slots, real KeyTable versus userspace-manager identity, and Domain.Grant's relationship to KeyTable operations.
- [ ] Specify per-kind copy/move/delete semantics and transactional transfer outcomes.

### Who tracks revocation?

Comments propose a userspace KeyMaster and flat kernel tables, but the canonical D2 trust boundary remains open.

| Model | Benefits | Costs |
|---|---|---|
| Exclusive trusted manager controls derivation | Smaller kernel derivation machinery | Every derivation/transfer path must respect exclusivity; manager failure requires recovery |
| Kernel tracks derivation scopes | Supports direct delegation between mutually distrustful components | Charged metadata, traversal, bounded cancellation and reclamation |
| Coarse allocation/delegation scopes | Simpler initial revocation mechanism | Broader disruption; less selective reclamation |

**Recommendation:** consider coarse, explicitly authorized scopes if their granularity fits the system. Keep allocation/delegation policy in userspace while the kernel enforces scope validity and safe reuse. This recommendation does not settle whether derivation is manager-exclusive.

Lazy epoch invalidation can make rejection of new invocations cheap. It does not find/retire existing mappings, cancel pending operations, or release backing by itself. A userspace derivation tree, a kernel generation, and hardware teardown solve different problems.

**OPEN / DECISION REQUIRED — D2:**

- [ ] Choose the derivation/revocation trust boundary and supported scope granularity.
- [ ] If manager-exclusive, specify how every copy/move/IPC transfer path preserves exclusivity and how manager failure is recovered.
- [ ] If kernel-tracked, define charged metadata, provenance retention, traversal bounds, and scope reuse protection.
- [ ] Define which mappings and pending operations belong to a revoked scope, including derivations/transfers racing with revoke.
- [ ] Implement invalidation and reclamation completion only after the selected scope and authority contracts are approved.

## 5. Direct userspace memory access is not a fallible invocation

### Current code and limitation

Included [`DcbView`](../libs/object/src/domain.rs) constructs references into shared mapped memory. The excluded [`Buffer` wrapper](../libs/object/src/buffer.rs) creates ordinary slices while independent unmapping remains possible.

A stale load cannot return a capability error: it may fault, observe recycled data, or violate Rust reference guarantees. Borrowing a wrapper does not prevent another capability/domain/manager from invalidating the mapping. WRITE permission is not exclusivity; READ permission is not a guarantee against external mutation.

| Alternative | Tradeoff |
|---|---|
| Raw mapping handles with explicitly unsafe access | Low wrapper complexity; caller bears documented safety obligations |
| Enforceable mapping lease/pin | Can protect validity, but delays or constrains reclamation; does not alone establish aliasing/external-mutation safety |
| Copied, fallible observations | Avoid exposed persistent references; may require an agreed query operation and syscall cost |
| Shared-memory-specific access protocols | Appropriate for atomics, producer/consumer data, DMA, or MMIO; not automatically ordinary Rust slices |

**OPEN / DECISION REQUIRED — D1/D3/D5/D6/D7:** forced independent revocation and long-lived safe references are in tension. Choose which guarantees each API actually provides.

- [ ] Define the mapping owner, unmap/revoke rules, lease/pin duration, and completion semantics before exposing safe references.
- [ ] Specify external mutation and synchronization rules independently of mapping permissions.
- [ ] Define buffer lifetime and input stability across blocked operations, including copying, pinning, or revalidation where appropriate.
- [ ] Keep arbitrary safe slice construction unavailable until validity, aliasing, and mutation guarantees are enforceable.

## 6. Domain identity and DCB observation

### Current code

[`DcbPages`](../kernel/nucleus/src/objects/domain.rs) release indexes allocation metadata before validating the full ID range. Lookup checks page availability, not allocated incarnation. Reuse has no generation protection.

[`Nucleus::current_domain_mut` and `current_dcb_mut`](../kernel/nucleus/src/objects/nucleus.rs) fall back to domain zero when no current domain is recorded. Missing identity therefore selects authority rather than producing an error. Domain-pool allocation, table ownership, and DCB allocation are not yet one coherent lifecycle.

The shared DCB has unresolved layout/stride and publication issues, including non-atomic identity fields that cannot be rewritten concurrently with readers under an unsupported reference model.

**Recommendation:** allocate private domain state, DCB identity, and table ownership coherently; reject absent/stale caller identity. Before domain reuse, retire waits, replies, donations, scheduler relationships, and other retained references.

Permanently mapped bounded DCB storage could simplify mapping lifetime, but not incarnation checks or publication safety. A version check alone does not legalize racing non-atomic Rust reads. Prefer sound value/snapshot accessors over unrestricted references where necessary.

**OPEN / DECISION REQUIRED — D3/D5, with D1/D7/D8 dependencies:**

- [ ] Choose DCB layout/stride/capacity, visibility, discovery, mapping lifetime, and identity-reuse protocol.
- [ ] Specify independently observable fields versus coherent snapshots and a Rust-sound publication mechanism.
- [ ] Define legal Activate/Suspend/Resume transitions, preserving blocked continuations and execution-budget requirements.
- [ ] Implement coherent domain allocation/retirement and bounds/allocation/incarnation checks.
- [ ] Reject absent caller identity instead of implicitly selecting domain zero.

## 7. Pending operations, replies, and Time

### Current code and reachability

The Endpoint, Reply, Notification, EventCount, and Time operation/object files are excluded sketches. They are activation blockers, not claims of supported syscall behavior.

- [`Endpoint`](../kernel/nucleus/src/objects/endpoint.rs) queues multiple callers but stores one endpoint-global pending message. Its two arrival orders do not implement equivalent reply/completion behavior.
- [`Reply`](../kernel/nucleus/src/objects/reply.rs) identifies a caller rather than a particular invocation and removes transferred authority before fallible destination installation.
- The excluded [`Reply` userspace wrapper](../libs/object/src/reply.rs) forgets ownership before checking success. [`Time`](../libs/object/src/time.rs) uses slot deletion in `Drop`; consuming wrappers do not yet have approved loan/transfer/failure semantics.

For a blocked caller, “the next invocation fails” is inadequate: there may never be another invocation. Retirement must arrange an explicit continuation outcome. Revocation/timeout cannot undo already committed effects or imply that a delivered request was never processed.

**Recommendation:** a bounded pending-invocation record owns payload, badge/authority metadata, phase, reservations, and eventual result. Both rendezvous arrival orders use one commit transition. Reply names that invocation; reply, timeout, cancellation, and teardown compete for exactly one terminal transition.

**OPEN / DECISION REQUIRED — D3/D7/D8/D9:**

- [ ] Define per-phase timeout/cancellation semantics, late replies, open/closed wait identities, and terminal results.
- [ ] Decide whether already-delivered calls remain replyable after endpoint retirement.
- [ ] Define explicit deferred completion and bounded wait/reply reservation, including domain/capability/resource teardown.
- [ ] Preserve source ownership on pre-commit failure; return it from consuming client APIs when retry remains possible.
- [ ] Represent irreversible partial completion explicitly, especially reply-committed/receive-failed.
- [ ] Make destructors best-effort fallback, not the sole guarantee of releasing callers or recovering budget; cleanup must not target replacement slot incarnations.
- [ ] Choose Time donation as loan versus transfer, unused-budget destination, provenance, deletion/expiry/cancellation outcomes, and multicore accounting.
- [ ] Implement one-shot reply consumption, race-free notification/event-count waits, and budget conservation on the shared completion foundation.

## 8. Memory reclamation and protection

### Current code

The excluded [`Frame` handler](../kernel/nucleus/src/api/arch/frame.rs) records mapping state without implementing the hardware transition. Included inline region mapping state in [`KeyEntry`](../kernel/nucleus/src/api/key_entry.rs) lacks enough context for general teardown.

The excluded [`Untyped` retype sketch](../kernel/nucleus/src/objects/untyped.rs) advances its watermark before subsequent fallible creation/insertion. The excluded Buffer handler can lose bookkeeping around partial map/unmap failures.

**Recommendation:** durable mapping identity, transactional creation, and retained partial-teardown bookkeeping. Before reuse, complete required CPU TLB/device translation synchronization and sanitize fresh ordinary RAM crossing protection boundaries. Intentional content-preserving sharing and device memory require distinct treatment.

The shared-address-space protection decision matters here: capability checks on syscalls cannot mediate arbitrary CPU loads/stores through installed translations. A common address namespace also does not imply that only one CPU can cache a translation.

**OPEN / DECISION REQUIRED — D1/D2/D4/D6:**

- [ ] Choose the protection backend/threat model and Domain/VSpace relationship without silently abandoning shared-namespace intent.
- [ ] Define physical and metadata layouts, accounting, single/batch retype, and initialization/sanitization enforcement.
- [ ] Define complete mapping identity, permissions/attributes, ASID ownership, and hardware-safe namespace reuse.
- [ ] Implement validate → reserve → initialize/prepare → commit for retype, mapping, and transfer, with rollback or explicit recoverable partial state.
- [ ] Retain teardown bookkeeping until mapping/device invalidation completes; do not reset an allocation watermark as a substitute for revoke.
- [ ] Implement backing reuse only after authority retirement, pending-use retirement, hardware synchronization, and required sanitization.

## 9. Complexity and suggested implementation order

Relative engineering estimates include focused tests. They are not measured performance claims or schedule commitments; scope restrictions and protection/multicore choices can substantially change them.

| Work | Complexity | Main cost |
|---|---|---|
| Thin fallible wrappers; preserve kernel errors | Small | Local client changes |
| Guarded typed-pool identity and reuse checks | Medium | Storage and access APIs |
| Incarnation-bearing userspace handles | Medium | Coordinated ABI, bootstrap, and wrapper migration |
| Minimal transactional KeyTable operations | Medium | Rights matrix, aliases, failure atomicity |
| Domain/DCB retirement and sound observation | Large | Shared-memory publication and cross-subsystem identity |
| Pending-operation and one-shot Reply foundation | Large | Cancellation races and scheduler integration |
| General selective revoke plus memory reclamation | Very large | Descendants, mappings, hardware, bounded completion |
| Safe revocable mapping abstractions / multicore Time | Very large | Protection and concurrency contracts |

Indexed identity validation can be constant-time in steady state. Full reclamation generally scales with affected capabilities, mappings, and pending operations; it must not masquerade as constant-time deletion. Coarse scopes and initially serialized execution can reduce implementation complexity, not eliminate semantic obligations.

Within the main implementation plan's dependency ordering, the recommended first decisions are:

- [ ] Settle whether a key names the current slot occupant or a particular incarnation.
- [ ] Settle who may retire a resource independently of deleting a capability.
- [ ] Settle manager-exclusive, kernel-tracked, or restricted coarse-scope derivation/revocation semantics.
- [ ] Settle what revoke completion guarantees about pending work and installed access.
- [ ] Settle which userspace APIs may expose direct references and what prevents invalidation.
- [ ] Record approved choices in the canonical contract and reconcile the implementation plan before enabling dependent operations.
- [ ] Implement guarded storage and the smallest approved KeyTable lifecycle first; leave unresolved revoke/derivation behavior unsupported.
- [ ] Follow with domain/DCB, memory reclamation, deferred completion/IPC, and Time slices according to their actual prerequisites, not by enabling existing sketches wholesale.

### Validation to carry out

Use the repository's documented Justfile workflows. Pure ABI/model tests do not substitute for target validation of mapping, hardware synchronization, or blocked-call resumption.

- [ ] Test stale slot/object/domain identities, same-type replacement, and generation exhaustion/reuse policy.
- [ ] Test same-table/same-object aliases, null insertion, capacity exhaustion, and bookkeeping consistency.
- [ ] Test rights attenuation, badge policy, explicit destination authority, and unauthorized retirement/revocation.
- [ ] Inject failures before and after reservation; verify move/transfer/retype ownership and accounting preservation.
- [ ] Test invoke/derive/transfer versus revoke ordering and the advertised revoke-completion boundary.
- [ ] Test pending-operation cancellation, both IPC arrival orders, late replies, exactly-once completion, and domain teardown/reuse.
- [ ] Test DCB layout, publication, mapping availability, identity reuse, and snapshot semantics.
- [ ] Test partial mapping/unmapping, TLB/device-safe reuse, and cross-boundary RAM sanitization on the relevant target.
- [ ] Test Time conservation, donor completion, deletion/expiry, and simultaneous-spending prevention once the budget contract is approved.
- [ ] Audit every enabled wrapper for observable failures; do not return fake success or lose authority on pre-commit error.

## Investigation boundary

At the investigation snapshot, the active dispatcher supports only the debug-gated console handler; Domain/KeyTable mutation clients preserve errors but their kernel handlers remain unsupported. Most other operation families above are excluded sketches. Storage, inline region helpers, and partial domain/DCB code are included, but inclusion is not proof of a supported end-to-end operation.

Sources of reachability: [`api/mod.rs`](../kernel/nucleus/src/api/mod.rs), [`objects/mod.rs`](../kernel/nucleus/src/objects/mod.rs), and [`libs/object/src/lib.rs`](../libs/object/src/lib.rs). Recheck these before acting on this snapshot.

The investigation ran no builds or tests. No implementation task or architectural decision is completed merely by documenting it.

**Summary recommendation:** keep userspace handles thin; put identity validation, authority enforcement, and retirement in the kernel. Add userspace ownership machinery where it protects slot management, transactional consumption, or direct memory access—not to mirror object liveness everywhere.
