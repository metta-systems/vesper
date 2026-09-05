# Lifetime and authority semantics

## Status and relationship to the capability contract

Design investigation and maintainer follow-up recorded on 2026-09-05. **This document distinguishes maintainer-confirmed semantics from research, proposals, and unanswered questions.** The confirmed subset is recorded in the canonical contract; neither documenting an alternative nor citing seL4/Composite adopts it as Vesper policy. No new wire encoding or runtime implementation is introduced here.

- [Nucleus capabilities](nucleus_capabilities.md) remains the canonical contract and decision register (D1–D9).
- [Capability implementation plan](capabilities_implementation_plan.md) remains the dependency-ordered implementation checklist. The tasks here expand the lifecycle/authority work; they do not replace its phase ordering or mark existing tasks complete.
- **Current code** describes the inspected implementation, not a desired contract. Recheck reachability and source details before implementing a slice.
- **CONFIRMED** identifies maintainer-selected semantics, also reflected in the canonical contract. **INTERIM** identifies a temporary direction whose detailed enforcement still needs decisions.
- **Recommendation** identifies a proposed direction, not an accepted decision. Historical alternatives are retained for context, not as competing implementation instructions.
- **OPEN / DECISION REQUIRED** identifies an unanswered question that must be settled before dependent implementation.
- All `- [ ]` items are outstanding decision, implementation, or validation work. Implementation tasks are conditional on the relevant decisions; alternative designs are not instructions to implement every option.

When a decision is approved, record its chosen semantics, rationale, and compatibility impact in the canonical contract first, then reconcile this document and the implementation plan before changing dependent code.

## Working hypothesis: thin handles, authoritative kernel checks

The motivating position is that userspace representations should not closely track kernel object lifecycle: after an object is disposed, invoking its userspace capability should return an error.

**CONFIRMED direction:** ordinary userspace handles need not monitor kernel object liveness; they name the capability incarnation obtained by the caller. The remaining qualification concerns mapped-memory APIs and precise completion contracts:

> Userspace handles need not mirror liveness, provided the kernel validates the intended identity and authority on each invocation. Direct memory access and ownership-consuming operations need additional contracts.

Not tracking whether an object is alive is different from not knowing which incarnation of an object or slot a handle names. A userspace liveness query is at most an observation; it cannot replace validation at the operation's authorization/commit boundary.

The desired separation is:

- A **handle** names the particular capability incarnation obtained by the caller, never an unrelated future occupant of its slot.
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

### Historical alternatives and selected semantics

The incarnation guarantee is now selected; a slot-only current-occupant API is not the chosen contract. The exact representation remains open.

| Choice | Consequences | Complexity |
|---|---|---|
| Slots name their current occupant, like file descriptors | Accept reuse hazards; userspace coordinates slot ownership | Low kernel complexity |
| Exclusive userspace slot allocator with owned slots and borrowed handles | Prevent accidental reuse while handles exist, provided every mutation follows that allocator | Medium; requires enforceable runtime discipline |
| Kernel-checked slot incarnation supplied with invocation | Retained handles fail after replacement without monitoring liveness | Medium cross-layer change, including ABI/bootstrap |

**CONFIRMED:** invocation through an altered or invalidated capability incarnation returns an explicit **inconsistency** error. Do not refresh a stale handle and retry against replacement authority silently. A userspace generation precheck alone leaves a check/use race; validation belongs in the invocation path. The error's numeric status/details and handle encoding are not yet defined.

Authorized resource-state changes are not automatically capability replacement. In particular, origin-authorized Frame remapping is a valid operation even when mapping location changes. Which other authority mutations create a new incarnation, and how a mutating call returns its replacement key, remain open.

**INTERIM Frame deprovisioning direction:** fully revoke the capability and do not reuse that slot for Frames. This is not a general solution to object/slot identity; the restriction's lifetime, cross-table scope, exhaustion behavior, and whether another kind may occupy the slot need clarification.

Three distinct identity problems must be covered:

- **Slot incarnation:** whether a local handle refers to a replaced entry.
- **Object/domain incarnation:** whether a kernel reference refers to recycled storage.
- **Selective delegation-scope validity:** relevant only if one branch of capabilities to a still-live object must be revoked while other branches survive.

**Correction to the initial analysis:** derived capabilities reference the same kernel object. An authoritative object-generation mismatch is sufficient to reject all old invocations of that retired object, without walking a capability ancestry chain. The branch distinction matters for selective revocation, not for detecting global object retirement. A generation is not a secret or a substitute for authorization, and object generation alone does not detect replacement of a slot with a different valid capability.

**OPEN / DECISION REQUIRED — D3/D9:**

- [ ] Implement the selected incarnation guarantee and define the shared inconsistency error without inventing an ad hoc status.
- [ ] Specify which authority mutations replace an incarnation; distinguish ordinary object operations and valid Frame remapping.
- [ ] Specify/enforce the interim Frame slot no-reuse restriction and its capacity/exhaustion policy.
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

### CONFIRMED: permission-based lifetime control and accepted leaks

`Untyped.Retype` yields creation-origin capabilities with permission to control the created object's lifetime. The permission may be delegated to derived capabilities, for example when granting an object to a resource manager. Authorization depends on capability permissions, not on a distinct owner-object type or privileged manager identity.

The creator retains control through its own capability until it destroys that capability. Donating object authority does not implicitly surrender the donor's control. Do not introduce a consuming client API that silently removes it. This statement concerns object delegation, not the unresolved CPU-budget semantics of Time.Donate.

Object retirement and subtree revocation are different operations: the kernel retires the object; KeyMaster separately initiates subtree revocation/cleanup, which can run in the background once old object invocations are rejected. The tree does not have to be erased synchronously for generation-based rejection to work.

Deleting the last retirement-authorized capability without retiring the resource may leak the retyped allocation forever. **That is accepted OS resource-management failure, not a kernel safety failure or a mandatory kernel recovery job.** Like a Rust memory leak, the allocation remains unavailable for conflicting reuse. Recovery/supervision policy, if desired, belongs to the OS.

The earlier suggestion to require orphan recovery or automatic final-capability retirement is withdrawn. Capability reference counting alone would in any event not account for pending invocations, mappings, or internal relationships.

A pin that keeps storage safe is **not** permission to continue using revoked authority. Physical lifetime and logical authorization must remain separate.

**OPEN / DECISION REQUIRED — D2/D3/D7/D8:**

- [ ] Define remaining per-kind deletion effects without imposing automatic final-capability retirement or mandatory leak recovery.
- [ ] Assign and enforce delegable lifetime-control permission for Retype origins and derived caps; distinguish it from subtree-management and origin-remap authority.
- [ ] Keep leak accounting/non-reuse correct; any manager-death recovery policy is an OS concern, not a prerequisite for kernel garbage collection.
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

### CONFIRMED: correctness before representation optimization

Do not optimize around the current sizes/layouts of kernel objects, keys, or capability entries. Add, remove, or expand fields and introduce shared lifetime/mapping records as needed to reach consistent correctness. Existing compact/inline representations and 32-byte KeyEntry layout are not fixed design constraints. A separate later optimization pass should use measurements to decide whether packing, compression, indirection removal, or other changes offer worthwhile gains.

Layout freedom does not remove layout obligations. Changes must update all affected calculations: physical versus metadata allocation, alignment, pool capacity/backing, table strides, DCB record/page views and page counts, initialization bounds, serialization/shared ABI consumers, and mandatory layout assertions/tests. Hardware-defined layouts and agreed wire IDs/encodings still require coordinated treatment; do not merely disable assertions to accommodate a new size.

- [ ] Introduce the identity, mapping, synchronization, and lifetime fields required for correctness without preserving obsolete size targets.
- [ ] Audit and update size-dependent allocation/capacity/stride/page-view calculations and cross-layer layout tests with each representation change.
- [ ] After correctness is established, measure memory/performance costs and perform a separate optimization pass without weakening the contracts.

### Proposed foundation

Typed pools remain a useful starting point, with representation free to change under the correctness-first rule:

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
- Copy is per-kind: Untyped cannot duplicate independent allocation watermarks; Reply cannot create independently usable replies; Time must conserve budget. **Latest confirmed Frame rule:** checked Copy duplicates capability authority, not a live mapping association; it is not Map. B maps the same physical frame through a separate Map operation, potentially at a different virtual page/Domain. This supersedes the earlier interpretation that Copy might install a mapping. A move preserves the binding needed for teardown.
- Validate and reserve a destination before removing source authority. Resolve aliases before constructing references or mutating operands.

**OPEN / DECISION REQUIRED — D4, plus D2/D3/D6/D8 where applicable:**

- [ ] Define per-kind semantic rights and the operation-to-rights matrix before freezing bit assignments.
- [ ] Define permitted attenuation of rights, extents, badges, budgets, and other per-kind authority.
- [ ] Set badge width, zero-badge meaning, and mint/rebadge/notification-bit policies.
- [ ] Define destination table management versus installation/receive authority.
- [ ] Finalize bootstrap slots, real KeyTable versus userspace-manager identity, and Domain.Grant's relationship to KeyTable operations.
- [ ] Specify per-kind copy/move/delete semantics and transactional transfer outcomes.

### Who tracks revocation?

**CONFIRMED:** trusted userspace KeyMaster alone manages derivation trees. The kernel supplies checked operations and object retirement, not general derivation-tree policy. The maintainer selects a **SPeCK-like resource-management model**: userspace policy and derivation management over kernel resource, liveness, and quiescence mechanisms. This is architectural direction, not wholesale adoption of Composite layouts, every implementation detail, or its address-space configuration. The table below preserves historical alternatives; kernel-owned tree management and a mandatory kernel ancestry chain are not the chosen direction.

| Model | Benefits | Costs |
|---|---|---|
| Exclusive trusted manager controls derivation | Smaller kernel derivation machinery | Every derivation/transfer path must respect exclusivity; manager failure requires recovery |
| Kernel tracks derivation scopes | Supports direct delegation between mutually distrustful components | Charged metadata, traversal, bounded cancellation and reclamation |
| Coarse allocation/delegation scopes | Simpler initial revocation mechanism | Broader disruption; less selective reclamation |

**Selected split:** KeyMaster tracks the tree; derived caps across tables reference a common object identity. Kernel object retirement changes that object's validity/generation, rejecting all previous cap invocations. KeyMaster then cleans up dead entries and derivation metadata in the background. Shallow trees are a performance expectation, not a requirement to traverse ancestry on every invocation.

An object-wide epoch does not provide selective branch-only invalidation while sibling caps to the same object remain valid. If that separate operation is required, KeyMaster must use an explicit invalidation protocol; the exact primitive/completion boundary remains open. Do not add kernel ancestry metadata as a presumed prerequisite for the already-selected object-retirement mechanism.

Lazy object invalidation does not remove PTEs or cancel all retained work by itself. **The trusted library OS owns unmap-before-invalidation orchestration.** It must not invalidate the only usable cleanup authority and then expect a recipient to cooperate. How premature physical reuse is rejected or prevented, and which checks are kernel-enforced versus trusted-manager preconditions, remains D2/D6. Accepted allocation leaks must not become conflicting physical reuse.

**OPEN / DECISION REQUIRED — D2:**

- [ ] Specify how every derivation/transfer path preserves the selected trusted-manager boundary, including Copy authorization and IPC registration.
- [ ] Implement shared-object validity checks across tables, and define stable authoritative metadata for inline Frame/Untyped regions as well as pooled objects.
- [ ] Define the kernel-retirement/KeyMaster-cleanup notification or handoff, idempotent cleanup, and generation-safe metadata reuse.
- [ ] Define selective subtree invalidation of a still-live shared object if required; do not assume the object-wide epoch preserves siblings.
- [ ] Define which mappings and pending operations belong to a revoked scope, including derivations/transfers racing with revoke.
- [ ] Implement invalidation and reclamation completion only after the selected scope and authority contracts are approved.

## 5. Direct userspace memory access is not a fallible invocation

### Current code and limitation

Included [`DcbView`](../libs/object/src/domain.rs) constructs references into shared mapped memory. The [`Buffer` wrapper](../libs/object/src/buffer.rs), intended functionality currently excluded from compilation, creates ordinary slices.

“Direct references” means `&[u8]`, `&mut [u8]`, or `&DomainControlBlock` produced by these userspace wrappers after mapping—not raw pointers granting authority across the kernel boundary. The control interface remains capability-based; subsequent loads/stores are not capability invocations and cannot return the inconsistency status.

`MappedSlice` and `MappedSliceMut` already invoke Unmap on Drop. **Intended ownership design:** create and own a private capability/mapping inside the guard, allow no further derivations or independently usable management aliases, and release the mapping on Drop. The current sketch does not yet implement this: `map_slice[_mut](&self)` borrows an existing BufferKey and stores that borrow rather than creating/owning a dedicated capability.

**CONFIRMED cleanup guarantee:** Drop uses an incarnation-bearing capability key and the necessary kernel checks. A stale key is rejected rather than allowed to unmap a replacement. This follows the general key contract, not a new destructor exception. Tests must establish it once the new keys exist; the current destructor discards errors. The private mapping design prevents another holder from independently remapping that same guard-owned association; its lifecycle must enforce this, rather than treating a normal remap as capability invalidation. A forgotten guard may leak, which is acceptable if it cannot cause invalid reuse.

There are two different exclusivity questions: ownership of the capability/mapping, and access to the physical bytes. No derivations from the guard-owned capability addresses the first. It does not prove that the same physical frame has no pre-existing aliases, ancestor retirement authority, or external writers. The full-borrow backing guarantee and integration with ancestor revocation/object retirement still need an explicit protocol.

**OPEN Rust mutability question:** WRITE permission is not exclusivity; READ permission is not a guarantee against external mutation. A mutable borrow of the guard is not automatically exclusive ownership of shared physical bytes. Whether to expose ordinary slices, scoped access guards, shared-memory-specific operations, or unsafe access remains undecided. Making a reference-producing API unsafe is an option, not the conclusion of this discussion; any unsafe contract must be enforceable for the entire borrow.

| Memory/access contract | Candidate Rust representation, not yet approved |
|---|---|
| Exclusively owned backing, with conflicting aliases/writers excluded for the borrow | Ordinary slices may be appropriate; mutable slices need physical-byte exclusivity, not just a private mapping |
| Shared producer/consumer or concurrently updated memory | Protocol-specific APIs, atomics/appropriate interior mutability, or synchronized access guards; do not expose unrestricted ordinary slices |
| Observation requiring a stable result rather than a live reference | Copy a synchronized snapshot into caller-owned memory |
| DMA or MMIO | Device-specific access, ordering/cache-coherency and volatile requirements as applicable, not automatic `&mut [u8]` |

Synchronization must cover every participant allowed to access the bytes. A userspace lock is insufficient if another authorized participant does not follow that protocol.

| Alternative | Tradeoff |
|---|---|
| Raw mapping handles with explicitly unsafe access | Low wrapper complexity; caller bears documented safety obligations |
| Enforceable mapping lease/pin | Can protect validity, but delays or constrains reclamation; does not alone establish aliasing/external-mutation safety |
| Copied, fallible observations | Avoid exposed persistent references; may require an agreed query operation and syscall cost |
| Shared-memory-specific access protocols | Appropriate for atomics, producer/consumer data, DMA, or MMIO; not automatically ordinary Rust slices |

**OPEN / DECISION REQUIRED — D1/D3/D5/D6/D7:** implement the private guard-owned mapping direction and investigate physical-byte exclusivity versus shared-resource semantics. Persistent DCB views and revocable buffer mappings need different contracts; do not assume both can disappear at arbitrary times.

- [ ] Implement dedicated guard-owned capability/mapping creation and teardown with no derivation or management-handle escape; replace the existing shared BufferKey borrow.
- [ ] Validate stale Drop rejection through the same incarnation checks as other invocations, and prevent independent remap of a live guard-owned association.
- [ ] Define full-borrow backing validity under ancestor revocation/object retirement; introduce leases/pins only if the selected contract requires them.
- [ ] Choose safe versus unsafe construction/access and define which physical-byte aliasing/writer guarantees each API establishes.
- [ ] Specify external mutation and synchronization rules independently of mapping permissions.
- [ ] Define buffer lifetime and input stability across blocked operations, including copying, pinning, or revalidation where appropriate.
- [ ] Keep arbitrary safe slice construction unavailable until validity, aliasing, and mutation guarantees are enforceable.

## 6. Domain identity and DCB observation

### Current code

[`DcbPages`](../kernel/nucleus/src/objects/domain.rs) release indexes allocation metadata before validating the full ID range. Lookup checks page availability, not allocated incarnation. Reuse has no generation protection.

[`Nucleus::current_domain_mut` and `current_dcb_mut`](../kernel/nucleus/src/objects/nucleus.rs) fall back to domain zero when no current domain is recorded. Missing identity therefore selects authority rather than producing an error. Domain-pool allocation, table ownership, and DCB allocation are not yet one coherent lifecycle.

The shared DCB has unresolved layout/stride and publication issues, including non-atomic identity fields that cannot be rewritten concurrently with readers under an unsupported reference model.

**Recommendation:** allocate private domain state, DCB identity, and table ownership coherently; reject absent/stale caller identity. Before domain reuse, retire waits, replies, donations, scheduler relationships, and other retained references.

**CONFIRMED direction:** DcbView is persistent and does not disappear like an ordinary buffer mapping. Record this rather than treating arbitrary DCB unmapping as a design requirement. The remaining questions are backing availability for each page, domain-record incarnation/reuse, publication, and snapshot consistency. A persistent view does not mean the observed domain lives forever. A version check alone does not legalize racing non-atomic Rust reads. Prefer sound value/snapshot accessors over unrestricted references where necessary.

**OPEN / DECISION REQUIRED — D3/D5, with D1/D7/D8 dependencies:**

- [ ] Choose DCB layout/stride/capacity, visibility, discovery, mapping lifetime, and identity-reuse protocol.
- [ ] Specify independently observable fields versus coherent snapshots and a Rust-sound publication mechanism.
- [ ] Define legal Activate/Suspend/Resume transitions, preserving blocked continuations and execution-budget requirements.
- [ ] Implement coherent domain allocation/retirement and bounds/allocation/incarnation checks.
- [ ] Reject absent caller identity instead of implicitly selecting domain zero.

## 7. Pending operations, replies, and Time

### Current code and reachability

The Endpoint, Reply, Notification, EventCount, and Time operation/object files are excluded because they do not compile as-is today. They remain intended functionality and must participate in lifecycle design now. Their current code is not evidence of supported syscall behavior; exclusion is not a reason to discard their design or omit future implementation.

- [`Endpoint`](../kernel/nucleus/src/objects/endpoint.rs) queues multiple callers but stores one endpoint-global pending message. Its two arrival orders do not implement equivalent reply/completion behavior.
- [`Reply`](../kernel/nucleus/src/objects/reply.rs) identifies a caller rather than a particular invocation and removes transferred authority before fallible destination installation.
- The excluded [`Reply` userspace wrapper](../libs/object/src/reply.rs) forgets ownership before checking success. [`Time`](../libs/object/src/time.rs) uses slot deletion in `Drop`; consuming wrappers do not yet have approved loan/transfer/failure semantics.

For a blocked caller, “the next invocation fails” is inadequate: there may never be another invocation. Retirement must arrange an explicit continuation outcome. Revocation/timeout cannot undo already committed effects or imply that a delivered request was never processed.

**Recommendation:** a bounded pending-invocation record owns payload, badge/authority metadata, phase, reservations, and eventual result. Both rendezvous arrival orders use one commit transition. Reply names that invocation; reply, timeout, cancellation, and teardown compete for exactly one terminal transition.

### CONFIRMED vocabulary for aborted work

| Outcome | Meaning |
|---|---|
| Rejected before admission | This attempt did not start |
| Cancelled before commit | The operation guarantees its defined commit did not occur; preparatory effects are not automatically erased |
| Completed | The outcome is known, including a known operation-specific failure or explicitly reported partial result |
| Outcome unknown | Work may have committed, but the observer cannot establish completion |

These terms are adopted for discussion and operation contracts; they are not new numeric statuses or an approved wire result type. Authority validity is independent of outcome: revocation or a missing reply cannot prove that previously delivered work did not commit. The operation must define its commit point before it can promise cancellation-before-commit.

**CONFIRMED boundary:** the nucleus provides local mechanisms only. It is not concerned with distributed or remote operations. Network services/libOS code may need request identities, retries, deduplication, durable outcome queries, or lease protocols; those are not kernel implementation tasks. A partitioned remote peer and a lost reply motivate “outcome unknown,” not new distributed responsibilities for the nucleus.

**OPEN / DECISION REQUIRED — D3/D7/D8/D9:**

- [ ] Define each local operation's commit point and which adopted outcome categories it can produce; select shared ABI encodings separately.
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

### CONFIRMED Frame mapping direction

- **Copy is not Map.** Checked Copy creates another permitted capability to the same physical frame, without duplicating a live mapping association or installing a PTE. Separate Map creates that cap's mapping.
- The same physical frame may be mapped at different virtual pages in different Domains through capabilities derived from the same origin. Their mapping associations are independent of the shared frame identity.
- Unmap is mapping-local, whether invoked through origin A or derived B; it does not itself traverse descendants.
- **Terminology correction:** the maintainer's earlier “origin Unmap” meant capability **Revoke on the origin**, as in seL4, not a stronger Frame.Unmap. Origin Revoke withdraws descendants while retaining the origin; removing the origin's own mapping/capability is separate. Earlier text describing an Unmap primitive that removes A+B was a misunderstanding and is superseded. KeyMaster implements tree policy using kernel mechanisms; object retirement is still distinct.
- Remapping is origin-authorized and remains a valid operation on the capability. The concrete effect on descendant mappings, and virtual relocation versus replacing physical backing, remain open.
- For now, deprovisioning requires full revocation and no reuse of that slot for Frames; scope and capacity consequences need definition.
- The libOS must perform the correct Unmap/retirement order. Kernel object invalidation rejects cap invocations; it is not automatic PTE teardown. KeyMaster's background cleanup is a separate responsibility.

The current inline `RegionPayload` has physical address and compressed per-cap mapping state, but no shared Frame lifetime generation or full target-context identity. Add the authoritative frame/allocation lifetime and mapping metadata needed for correctness, freely changing object/key sizes or introducing shared records. Keeping extent metadata inline is optional, not a constraint. Audit size-dependent calculations and views whenever these layouts change, then optimize in a later measured pass.

**OPEN / DECISION REQUIRED — D1/D2/D4/D6:**

- [ ] Choose the protection backend/threat model and Domain/VSpace relationship without silently abandoning shared-namespace intent.
- [ ] Define physical and metadata layouts, accounting, single/batch retype, and initialization/sanitization enforcement.
- [ ] Define complete per-mapping identity, permissions/attributes, ASID ownership, and hardware-safe namespace reuse.
- [ ] Specify/implement separate Copy and Map schemas: Copy installs only the destination capability; Map validates target context, address, rights, and mapping state before installing a PTE.
- [ ] Specify origin-capability Revoke orchestration, descendant mapping withdrawal, failure/partial completion, and prevention of racing remaps/derivations; keep local Unmap and origin removal distinct.
- [ ] Specify virtual remap versus physical-backing replacement and whether derived mappings stay put, follow, or are withdrawn; origin permission alone is not a userspace pointer-lifetime proof.
- [ ] Encode/enforce origin-only remap independently of delegable lifetime-control permission; a derived retirement-authorized capability does not thereby gain remap permission.
- [ ] Define libOS Unmap-before-invalidation prerequisites and the kernel checks/trusted-manager obligations that prevent premature reuse.
- [ ] Implement validate → reserve → initialize/prepare → commit for retype, mapping, and transfer, with rollback or explicit recoverable partial state.
- [ ] Retain teardown bookkeeping until mapping/device invalidation completes; do not reset an allocation watermark as a substitute for revoke.
- [ ] Implement backing reuse only after authority retirement, pending-use retirement, hardware synchronization, and required sanitization.

### Different virtual pages and the single-address-space goal

**CONFIRMED use case:** two derived capabilities to physical frame F can Map it at X in Domain A and Y in Domain B. This does not require copying F or changing its object identity.

**OPEN — D1/D6:** distinguish three properties that are often conflated:

1. One shared virtual-address namespace/allocation policy.
2. One hardware translation root/page-table view.
3. Identical access permissions for every Domain.

The intended aliasing use case can coexist with a shared namespace if X and Y are globally meaningful aliases of F. A shared namespace does not require exactly one virtual address per physical frame. What conflicts with a strict shared-meaning namespace is giving the same address X unrelated meanings in different Domains, not giving one frame multiple addresses.

| Candidate | Consequences and tradeoffs |
|---|---|
| Global virtual allocation with per-Domain protected views | Where an address is present, it has the same intended backing; views may omit aliases or restrict rights. Supports X/Y aliases and shared-pointer conventions, but needs coordinated allocation and can still require translation-context switches |
| One shared translation root | X and Y can both map F, but ordinary PTE permissions alone do not distinguish Domains executing with the same relevant privilege/root. Separate domain protection machinery or a restricted trust model is required |
| Independent Domain-local virtual allocation | Straightforward per-Domain X/Y placement, but the same numerical pointer need not retain meaning across Domains; this changes the strict shared-namespace goal and is not silently adopted |

**Discussion recommendation, not a D1 decision:** explore global virtual allocation plus protected per-Domain views first. It separates a common namespace from identical mappings/access. Applications that need cross-Domain raw pointers may use a canonical shared address; aliases require pointer translation or offset-based payloads as appropriate. Choosing SPeCK-like lifetime/resource mechanisms does not itself resolve these address-space questions or eliminate switching costs.

- [ ] Define whether shared address space means shared numerical meanings, shared translation roots, or both, and how X/Y aliases are allocated/discovered.
- [ ] Define target-context authority and which Domain can access each alias; choose the actual protection backend rather than infer isolation from capability lookup.
- [ ] Define shared-buffer pointer versus offset conventions when peers map the same backing at different addresses.
- [ ] Test cross-Domain aliases, independent mapping-local Unmap, and descendant withdrawal by origin cap-Revoke under the selected backend.

### seL4 and Composite research: mapping authority versus object lifetime

Primary documentation and source inspected on 2026-09-05. seL4 evidence is pinned to **16.0.0, AArch64** where implementation-specific. Composite evidence distinguishes the older memory-manager interface from SPeCK/current kernel mechanisms. These comparisons inform Vesper; they do not override the confirmed semantics above.

#### seL4: one tracked mapping per frame cap, Copy then Map

The [mapping tutorial](https://docs.sel4.systems/Tutorials/mapping.html#pages) and [16.0.0 VSpace manual](https://github.com/seL4/seL4/blob/16.0.0/manual/parts/vspace.tex) describe sharing by copying a frame capability and then mapping the copy in another VSpace. Physical contents are not copied.

In [AArch64 `Arch_deriveCap`](https://github.com/seL4/seL4/blob/16.0.0/src/arch/arm/64/object/objecttype.c), deriving a frame cap clears its mapped ASID. Thus the new cap has no tracked active mapping; Copy alone does not install a destination PTE. It retains the frame identity and appropriately attenuated capability rights, not necessarily the source PTE's current narrower permissions.

[AArch64 frame invocation](https://github.com/seL4/seL4/blob/16.0.0/src/arch/arm/64/kernel/vspace.c) establishes the following:

| Operation | seL4 16.0.0 AArch64 behavior |
|---|---|
| Map an unmapped frame cap | Establish a mapping with the supplied VSpace authority and validated rights/attributes |
| Map an already mapped cap at the same ASID/virtual address | May update its mapping rights/attributes, subject to checks |
| Map an already mapped cap at a different ASID or virtual address | Rejected; unmap before relocation |
| Page_Unmap on A or B | Remove only that cap's recorded mapping; clear its mapping state, leaving the frame cap usable |
| Delete a mapped frame cap | Finalization unmaps that cap's recorded mapping, even if other frame caps survive |

The separate-PTE assumption matters: two caps aimed at the same PTE do not create independent hardware mappings. The AArch64 mapping code also does not reject every occupied leaf destination, so generic API error descriptions must not be generalized into an unconditional overwrite prohibition.

**Origin distinction:** seL4 does recognize original capabilities in its [capability derivation tree](https://github.com/seL4/seL4/blob/16.0.0/manual/parts/cspace.tex), but not as an origin-only permission for Frame Map/Unmap. For ordinary original frame A and derived B, further copies of B are derived siblings rather than arbitrary nested revocation roots. CNode_Revoke(A) deletes its descendants, with mapping cleanup, but retains A and A's distinct mapping. Page_Unmap(A) is not that operation: it leaves B's distinct mapping intact. Deleting A alone does not recursively delete B.

Sources: [`CNodeCopy`, `cteRevoke`, and MDB handling](https://github.com/seL4/seL4/blob/16.0.0/src/object/cnode.c), [`Arch_finaliseCap`](https://github.com/seL4/seL4/blob/16.0.0/src/arch/arm/64/object/objecttype.c). seL4's ordinary object-lifetime model uses final capability deletion, not Vesper's explicit permission-based retirement with acceptable abandoned allocations. Covering Untyped revocation removes its descendants; subsequent Retype can reset/reuse the region once no children remain. Retyping with remaining children uses the watermark rather than reclaiming arbitrary holes. General-purpose RAM is cleared on reset; device memory is not. See [object/memory manual](https://github.com/seL4/seL4/blob/16.0.0/manual/parts/objects.tex) and [`untyped.c`](https://github.com/seL4/seL4/blob/16.0.0/src/object/untyped.c).

**Lesson for Vesper:** the maintainer now adopts Copy-not-Map and clarifies that broad origin withdrawal means cap-Revoke, not Page_Unmap. The earlier claimed difference on those points is superseded. seL4 remains useful for per-cap mapping identity and the local-Unmap/origin-Revoke distinction. Its kernel-managed derivation, automatic deletion cleanup, final-cap lifetime rules, and remap authority need not be adopted: Vesper selects SPeCK-like manager/mechanism separation, permission-based object retirement, and accepted leaks.

#### Older Composite: userspace mapping tree and recursive release

The legacy [memory-manager interface](https://github.com/gwsystems/composite/blob/043980416d660da1e0910549aa3600e6b59ed055/src/components/interface/mem_mgr/mem_mgr.h) and [naive manager implementation](https://github.com/gwsystems/composite/blob/043980416d660da1e0910549aa3600e6b59ed055/src/components/implementation/mem_mgr/naive/mem_man.c) make the userspace responsibility concrete:

| Operation with mapping A → B, B a leaf | Effect |
|---|---|
| mman_alias_page(A, B) | Install a child mapping to the same frame |
| mman_release_page(B) | Remove B, retain A |
| mman_release_page(A) | Remove A and B |
| mman_revoke_page(A) | Remove descendants such as B, retain A |

Userspace `struct mapping` contains the frame, component/address, and parent/child/sibling links. `mapping_del_children()` walks descendants; `mapping_del()` removes descendants and then the selected mapping. A non-leaf B release removes B's descendants too.

The kernel operation named `COS_MMAP_REVOKE` removes one specified component/address mapping; it is not the recursive tree operation. See [legacy kernel implementation](https://github.com/gwsystems/composite/blob/043980416d660da1e0910549aa3600e6b59ed055/src/kernel/inv.c#L2841-L2912).

**Lesson for Vesper:** the userspace recursion is useful precedent, but preserve the operation distinction: manager revoke retains the root, whereas release removes root and descendants. The maintainer's broad origin operation is cap-Revoke; it is not a special hardware origin-Unmap mode. Neither recursive operation is established here as an automatic side effect of merely changing a shared object generation.

#### SPeCK/current Composite: liveness epochs plus separate mapping/reuse checks

The primary [SPeCK paper, RTAS 2015](https://www2.seas.gwu.edu/~gparmer/publications/rtas15speck.pdf), sections IV-A/B and IV-E/F, assigns delegation/revocation policy to userspace management components and separates individual copy/deactivate mechanisms from higher-level management. It distinguishes kernel-reference quiescence from TLB quiescence; invalidation is not immediate physical reuse.

Current source examined at [`3ef8f8c4d3296624640e6f3bd00801054d8350a3`](https://github.com/gwsystems/composite/commit/3ef8f8c4d3296624640e6f3bd00801054d8350a3):

- [`liveness_tbl.h`](https://github.com/gwsystems/composite/blob/3ef8f8c4d3296624640e6f3bd00801054d8350a3/src/kernel/include/liveness_tbl.h) pairs stored `(id, epoch)` references with authoritative epochs/deactivation timestamps. Expiration changes the epoch; a concrete use is [component deactivation](https://github.com/gwsystems/composite/blob/3ef8f8c4d3296624640e6f3bd00801054d8350a3/src/kernel/include/component.h), checked by relevant [invocation paths](https://github.com/gwsystems/composite/blob/3ef8f8c4d3296624640e6f3bd00801054d8350a3/src/kernel/include/inv.h). This is precedent for stale-object rejection, not proof of one universal generation scheme for every object kind.
- [`chal_pgtbl_cpy` and mapping deletion](https://github.com/gwsystems/composite/blob/3ef8f8c4d3296624640e6f3bd00801054d8350a3/src/platform/i386/chal_pgtbl.c) copy the source frame address into a distinct destination mapping and remove individual mappings. **Inference from these operations:** remapping A does not make B follow A's new mapping; B keeps its independently installed translation until explicitly changed/removed by the manager.
- Removed PTEs carry quiescence metadata before slot reuse. [`retypetbl_retype2frame`](https://github.com/gwsystems/composite/blob/3ef8f8c4d3296624640e6f3bd00801054d8350a3/src/kernel/retype_tbl.c) checks mapping accounting and quiescence before frame retyping. Userspace ownership of the mapping tree therefore coexists with kernel reuse checks.
- [`cap_cpy`](https://github.com/gwsystems/composite/blob/3ef8f8c4d3296624640e6f3bd00801054d8350a3/src/kernel/capinv.c#L249-L312) is type-specific; some copied table capabilities retain parent pointers/refcounts. Composite does not justify an absolute claim of no kernel dependency metadata, even though mapping-tree policy lives in userspace.

**Limits:** component expiration does not itself sweep every PTE; the examined source does not establish Vesper's unmap-before-invalidation protocol or a universal background subtree worker. Some cleanup paths remain incomplete, including [`cos_mem_remove`](https://github.com/gwsystems/composite/blob/3ef8f8c4d3296624640e6f3bd00801054d8350a3/src/components/lib/kernel/cos_kernel_api.c#L1596-L1601). This was source/document inspection, not execution or a correctness audit; paper goals and current implementation coverage must not be conflated.

#### Implications and remaining implementation questions

- Shared-object generation validation and KeyMaster-owned mapping trees are complementary, not competing mechanisms. No kernel ancestry walk is needed merely to reject all caps to a retired object.
- Neither system demonstrates that changing one origin mapping automatically retargets already-installed aliases. Vesper must define that remap effect explicitly; origin-only authority controls who may request it, not how existing translations or Rust borrows change.
- SPeCK is the selected architectural reference for resource management, with older Composite providing a concrete userspace mapping-tree example. An origin cap-Revoke can stop new descendant installations, enumerate/remove affected mappings, complete required synchronization, and invalidate/clean up descendant authority; retain origin authority unless a separate operation removes it. This is a proposed orchestration sequence, not an approved transaction/partial-completion schema.
- Keep object retirement, selective subtree revocation, mapping release, and physical reuse separate in API descriptions. The trusted libOS owns correct sequencing; whether and how the kernel rejects unsafe premature reuse is still a Vesper design decision, with Composite accounting/quiescence checks as a concrete option.

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

Indexed capability/object identity validation can be constant-time in steady state; retiring the shared object does not inherently require an ancestry walk. Full reclamation scales with affected mappings, pending operations, and cleanup records. Recursive manager unmapping can cost O(number of affected mappings), independently of later background cap-slot cleanup. Initial serialization can simplify the implementation without erasing those responsibilities. The kernel is not required to detect or recover allocation leaks.

Within the main implementation plan's dependency ordering, the next work is:

- [ ] Implement incarnation-bearing keys, authoritative shared-object lifetime validation, and the agreed inconsistency behavior.
- [ ] Implement delegable lifetime-control permission while preserving creator authority and accepted leak behavior.
- [ ] Enforce KeyMaster's derivation boundary and implement the retirement/background-cleanup handoff.
- [ ] Settle remaining selective-revocation and completion guarantees, including Unmap-before-invalidation and safe physical reuse.
- [ ] Implement private guard-owned mapping lifecycles, investigate Rust mutability/shared-resource APIs, and define persistent DCB record guarantees.
- [ ] Resolve how cross-Domain frame aliases fit the shared-address-space/protection model.
- [ ] Keep remaining decision approvals, schemas, and implementation evidence synchronized with the canonical contract and plan.
- [ ] Implement guarded storage and the smallest approved KeyTable lifecycle first; leave unresolved revoke/derivation behavior unsupported.
- [ ] Follow with domain/DCB, memory reclamation, deferred completion/IPC, and Time slices according to their actual prerequisites, not by enabling existing sketches wholesale.

### Validation to carry out

Use the repository's documented Justfile workflows. Pure ABI/model tests do not substitute for target validation of mapping, hardware synchronization, or blocked-call resumption.

- [ ] Test stale slot/object/domain identities, same-type replacement, and generation exhaustion/reuse policy.
- [ ] Test one object retirement rejecting old derived caps in multiple tables while background tree cleanup has not yet run; distinguish selective branch revocation.
- [ ] Test creator control surviving delegation, unauthorized retirement, and safe leaked allocations without automatic final-capability retirement.
- [ ] Test same-table/same-object aliases, null insertion, capacity exhaustion, and bookkeeping consistency.
- [ ] Test rights attenuation, badge policy, explicit destination authority, and unauthorized retirement/revocation.
- [ ] Inject failures before and after reservation; verify move/transfer/retype ownership and accounting preservation.
- [ ] Test invoke/derive/transfer versus revoke ordering and the advertised revoke-completion boundary.
- [ ] Test pending-operation cancellation, both IPC arrival orders, late replies, exactly-once completion, and domain teardown/reuse.
- [ ] Test DCB layout, publication, mapping availability, identity reuse, and snapshot semantics.
- [ ] Test partial mapping/unmapping, TLB/device-safe reuse, and cross-boundary RAM sanitization on the relevant target.
- [ ] Test Copy creating no mapping, separate Map in different Domains/virtual pages, local Unmap versus origin cap-Revoke, origin-only remap, interim Frame slot non-reuse, and generation-checked stale guard cleanup.
- [ ] Test guard-owned cap privacy/no derivation, ancestor-retirement integration, and the selected physical-byte exclusivity/shared-mutation contract.
- [ ] Test size/alignment/capacity/stride and DCB page-view calculations after correctness-driven object/key layout changes.
- [ ] Test the selected aborted-work categories against operation commit/cancellation boundaries; do not add network policy tests to the nucleus.
- [ ] Test Time conservation, donor completion, deletion/expiry, and simultaneous-spending prevention once the budget contract is approved.
- [ ] Audit every enabled wrapper for observable failures; do not return fake success or lose authority on pre-commit error.

## Investigation boundary

At the investigation snapshot, the active dispatcher supports only the debug-gated console handler; Domain/KeyTable mutation clients preserve errors but their kernel handlers remain unsupported. Most other operation families above are excluded because they do not compile as-is. They remain part of the design and planned implementation, not rejected functionality. Storage, inline region helpers, and partial domain/DCB code are included, but inclusion is not proof of a supported end-to-end operation.

Sources of reachability: [`api/mod.rs`](../kernel/nucleus/src/api/mod.rs), [`objects/mod.rs`](../kernel/nucleus/src/objects/mod.rs), and [`libs/object/src/lib.rs`](../libs/object/src/lib.rs). Recheck these before acting on this snapshot.

The investigation ran no builds or tests. No implementation task or architectural decision is completed merely by documenting it.

**Summary recommendation:** keep userspace handles thin; put identity validation, authority enforcement, and retirement in the kernel. Add userspace ownership machinery where it protects slot management, transactional consumption, or direct memory access—not to mirror object liveness everywhere.
