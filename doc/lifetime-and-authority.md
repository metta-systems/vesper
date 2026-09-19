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

### Historical counterexample and current status

The original [`Key<T>`](../libs/object/src/key.rs) contained only `KeySlot` and `PhantomData<T>`. Its slot-only transport allowed this counterexample:

1. Slot 12 contains endpoint A.
2. Userspace retains a key naming slot 12.
3. A's entry is deleted and the slot is reused for endpoint B.
4. The retained key invokes slot 12 and can operate on B.

Even a type check cannot distinguish two endpoints. This need not escalate authority across domains—B is already in the caller's table—but it can use replacement authority unintentionally. Thus stale invocation did not necessarily fail under the former slot-only representation.

Implemented follow-up (2026-09-07): RawKey now carries slot/incarnation across included wrappers, transport and active dispatch. Table lookup/removal checks the expected incarnation; deletion retains its counter and replacement advances it only at commit. Tests cover same-kind replacement, stale removal, failed installation, null/occupied/bounds rejection, ownership preservation and slot-local exhaustion. Debug bootstrap hands over the actual issued key through a private one-shot EL1 bridge; the real SVC success/error path passes the bounded boot smoke test. This fixes slot reuse within the table lifetime, not independent object/domain retirement or reusable-metadata identity. Full evidence and limits are in the plan's key wire and slot-identity slice.

### Historical alternatives and selected semantics

The incarnation guarantee is now selected; a slot-only current-occupant API is not the chosen contract. **Maintainer clarification (2026-09-06):** ordinary keys are local to the current Domain's implicit KeyTable and carry slot plus incarnation, not table identity or a guarded-table path. Numeric key transfer alone conveys no authority; authorized derivation/installation produces a recipient-local key. **Follow-up:** initially use a 32-bit slot and 32-bit incarnation, without freezing the total key size permanently. This replaces the earlier table-capacity-dependent bit split. If type is embedded later, its 8 bits come from the slot field (24 slot bits, 32 incarnation bits), not incarnation. Capacity is fixed for now through an easily changed constant; runtime resizing is outside initial scope. Follow-up approval (2026-09-07): pack slot in low 32 bits and incarnation in high 32 bits, with no initial type tag. Typed handles are non-owning. Live Domains may not rebind to a fresh logical invocation table with reset counters. Kernel allocation references use pool identity/index and 64-bit generation, with no generation wrap or metadata-identity reset; concrete ownership and Domain integration remain work. Independent shared-object lifetime validation remains required. Generation wrap must not silently resurrect stale identities; Move invalidates the source on commit and yields a destination-local key. The included ABI/bootstrap paths have migrated together without a slot-only fallback; excluded sketches remain unmigrated, unsupported design material.

| Choice | Consequences | Complexity |
|---|---|---|
| Slots name their current occupant, like file descriptors | Accept reuse hazards; userspace coordinates slot ownership | Low kernel complexity |
| Exclusive userspace slot allocator with owned slots and borrowed handles | Prevent accidental reuse while handles exist, provided every mutation follows that allocator | Medium; requires enforceable runtime discipline |
| Kernel-checked slot incarnation supplied with invocation | Retained handles fail after replacement without monitoring liveness | Medium cross-layer change, including ABI/bootstrap |

**CONFIRMED:** invocation through an altered or invalidated capability incarnation returns an explicit **inconsistency** error. Do not refresh a stale handle and retry against replacement authority silently. A userspace generation precheck alone leaves a check/use race; validation belongs in the invocation path. **Follow-up (3C=B):** one shared inconsistency error carries diagnostic reasons for stale slot incarnation, capability invalidation and underlying object retirement, without returning replacement keys or refreshing automatically. The approved key package now assigns InvalidKey/InconsistentKey/KeySlotExhausted statuses 26–28 and explicit reason/operand details, with key-shape/bounds/never-issued checks preceding incarnation mismatch, invalidation and object retirement. See the canonical contract for exact encoding and migration requirements.

Authorized resource-state changes are not automatically capability replacement. In particular, origin-authorized Frame remapping is a valid operation even when mapping location changes. **Selected initial scope (3A=A):** no in-place rights/badge mutation. Derive attenuated authority into another slot and optionally delete the original; CopyDerive preserves badges and rebadging is deferred. Future in-place mutation semantics are not a prerequisite for this initial slice.

**Selected exhaustion behavior (3B=A):** a slot unable to issue a fresh 32-bit incarnation is permanently unavailable for further installation within that table lifetime; existing-capability deletion remains possible and other slots remain usable. No silent wrap or whole-table retirement is implied. The approved key package starts unused counters at 0, issues 1 on first installation, advances only at successful installation, retains counters on deletion, and reports exhaustion with status 28. Replacing/rebinding the table must not reset stale-key protection.

**INTERIM Frame deprovisioning direction:** fully revoke the capability and do not reuse that slot for Frames. This is not a general solution to object/slot identity; the restriction's lifetime, cross-table scope, exhaustion behavior, and whether another kind may occupy the slot need clarification.

Three distinct identity problems must be covered:

- **Slot incarnation:** whether a local handle refers to a replaced entry.
- **Object/domain incarnation:** whether a kernel reference refers to recycled storage.
- **Selective delegation-scope validity:** relevant only if one branch of capabilities to a still-live object must be revoked while other branches survive.

**Correction to the initial analysis:** derived capabilities reference the same kernel object. An authoritative object-generation mismatch is sufficient to reject all old invocations of that retired object, without walking a capability ancestry chain. The branch distinction matters for selective revocation, not for detecting global object retirement. A generation is not a secret or a substitute for authorization, and object generation alone does not detect replacement of a slot with a different valid capability.

**OPEN / DECISION REQUIRED — D3/D9:**

- [ ] Implement the selected incarnation guarantee and define the shared inconsistency error without inventing an ad hoc status.
  - [x] Implement and validate the slot-incarnation guarantee and shared error schema through host tests, QEMU storage/dispatch tests, real debug boot/SVC smoke test, full Clippy and full `just test`. Independent object lifetime validation remains pending.
- [ ] Implement derivation-only authority attenuation for the initial slice, with no in-place rights/badge mutation; preserve ordinary object operations and valid Frame remapping as distinct from capability replacement.
- [ ] Specify/enforce the interim Frame slot no-reuse restriction and its capacity/exhaustion policy.
- [ ] Implement the selected implicit caller-table scope and receiver-local key creation through authorized derivation/installation; specify management operand/result encodings, constant-controlled capacity consumers, replacement/rebinding safety, and initial handoff; do not introduce runtime resizing in this slice.
- [ ] Define object/domain allocation identities and their authoritative validation metadata.
- [ ] Implement the approved packed 32-bit slot/32-bit incarnation fields without an initial type tag, increment-on-install, retained deletion counters and slot-local exhaustion. Implement the approved independent 64-bit allocation-generation model, metadata reuse rules and no-live-rebinding guarantee; integrate Domain identity coherently.
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
- `ObjectRef` (formerly `objects/object_ref.rs`) accepted `&T`, erased its lifetime, was copyable, and manufactured shared or mutable references based on a type tag. It was removed on 2026-09-07; it had no live call sites. The same lifetime-erasure hazard remains live in `KeyEntry::as_object`/`as_object_mut`, which dereference the entry's raw pointer directly.
- [`ObjectPool`](../kernel/nucleus/src/objects/object_pool.rs) tracks allocation bits and indexes, not incarnations or retirement.
- [`KeyTable`](../kernel/nucleus/src/objects/key_table.rs) now rejects null/occupied/out-of-bounds/exhausted insertion without changing occupancy or losing the submitted entry. Lookup/removal requires an incarnation-bearing selector, counters persist after deletion, and unrestricted mutable entry access is removed. The boot Domain pool is now carved from the boot Untyped's unused watermark range by Kickstart (2026-09-12); KeyTables are carved kernel-private objects created by runtime `Untyped.Retype` (2026-09-14); general pools still need authoritative lifetime metadata and full Untyped-backed ownership.
- The nucleus entry path uses [`IRQSafeNullLock`](../libs/locking/src/lib.rs), which masks interrupts rather than providing general multicore exclusion.

Returning an error requires safe validation **before** dereferencing an object. Comparing against metadata in already-freed backing is not a valid generation check.

### CONFIRMED: correctness before representation optimization

Do not optimize around the current sizes/layouts of kernel objects, keys, or capability entries. Add, remove, or expand fields and introduce shared lifetime/mapping records as needed to reach consistent correctness. Existing compact/inline representations and 32-byte KeyEntry layout are not fixed design constraints. A separate later optimization pass should use measurements to decide whether packing, compression, indirection removal, or other changes offer worthwhile gains.

Layout freedom does not remove layout obligations. Changes must update all affected calculations: physical versus metadata allocation, alignment, pool capacity/backing, table strides, DCB record/page views and page counts, initialization bounds, serialization/shared ABI consumers, and mandatory layout assertions/tests. Hardware-defined layouts and agreed wire IDs/encodings still require coordinated treatment; do not merely disable assertions to accommodate a new size.

- [ ] Introduce the identity, mapping, synchronization, and lifetime fields required for correctness without preserving obsolete size targets.
- [ ] Audit and update size-dependent allocation/capacity/stride/page-view calculations and cross-layer layout tests with each representation change.
- [ ] After correctness is established, measure memory/performance costs and perform a separate optimization pass without weakening the contracts.

### Selected guarded-access direction and remaining representation work

**CONFIRMED (2026-09-06):** enforce single-core kernel execution for the initial foundation and defer SMP. An owning kernel access context provides short-lived guarded references; pending work keeps checked identities/reservations. All managed storage/metadata originates from accounted Untypeds. When Retype creates a KeyTable, the kernel makes the backing private, including against the caller's direct access; the caller receives manipulation authority through a capability. **Retype clarification (5):** only an Untyped is a valid source; its unused watermark range has no outstanding access. Retype does not reuse an arbitrary live Frame/object or need to unmap earlier allocations. Enforce non-overlap and allocation provenance, then initialize the new KeyTable privately. Earlier allocations below the watermark may have their own live mappings; they are not the candidate allocation. Stable metadata placement, pool-backing ownership and transactional failure handling still need design. Any future reclamation/reset must complete access withdrawal before making a range available again, not turn ordinary Retype into a cleanup protocol. **Retype transaction status (2026-09-14):** runtime `Untyped.Retype` implements validate → reserve → pre-validate → initialize/install → watermark-last commit with defensive rollback and encoding-granularity-preserving watermarks (`nucleus::api::untyped`); stable metadata placement for kinds beyond the KeyTable/Frame allowlist and pool-backing ownership remain open (Frame joined the creatable allowlist 2026-09-15 with inline metadata and kernel-zeroed contents at retype). Device sources are rejected before reservation: no creatable kind is device-capable yet (per-kind device policy is D6). Extents are validated for representability (unrepresentable sizes/extents rejected with `InvalidSize`), and the absolute carve address (base + watermark) is aligned, not just the watermark.

Typed pools remain a useful starting point, with representation free to change under the correctness-first rule:

1. Persistent capabilities carry checked object identities, conceptually `(pool, index, generation)`.
2. Authoritative allocation metadata records allocation, generation, and retirement state, with its own valid backing lifetime.
3. An owning access context resolves identities and provides short-lived guarded access.
4. Multi-object operations resolve same-table/same-object aliases explicitly.
5. Object references and incompatible guards end before scheduling or context switching.
6. Pending operations retain checked identities and explicit reservations, not borrowed Rust references.

**SELECTED (2026-09-07) — D3 concrete guarded access:** capabilities store only a checked object identity (pool tag, pool index, allocation generation); entries hold no raw object pointer. The owning access context computes object addresses from pool bases after validating per-pool authoritative slot metadata (allocation state + generation + retirement), whose backing is stable and independent of object storage — validation always precedes dereference. The context is constructed once per invocation under the kernel lock, is `!Send`, and hands out guards that borrow it, so the borrow checker enforces guards ending before scheduling. Multi-operand operations use explicit pair-resolution forms that reject aliased mutable operands up front; a lock serializes executions but does not by itself prevent two operands within one execution from naming the same object. Mutable resolution exclusively borrows the pool for the invocation, so the borrow checker prevents holding two mutable guards into one pool; clients requiring two objects from the same pool must request them through the pair-resolution API (`resolve_pair_mut`) rather than sequential mutable resolution. One pool per type tag is the invariant making this sufficient; introducing multiple pools per type requires revisiting cross-pool alias enforcement (for example a runtime alias set). Single-core remains selected; SMP is not an open choice. **Remaining D3 work:** Untyped-backed pool ownership, kernel-private KeyTable backing, and safe pool-backing lifetime (Phase 5, with D1/D6 for protection bindings); domain lifecycle integration; per-kind access details. (2026-09-12: the carve primitives `carve_region`/`carve_pool` and the boot Untyped allocation are established in Kickstart's `bootstrap.rs`, carving the initial `Nucleus`, its boot Domain pool, and a boot KeyTable pool from the boot Untyped's unused watermark range; the boot Domain's table lives in that pool, and syscall entry now resolves the caller's table through the `Access` context. Runtime kernel-private KeyTable backing and full pool/metadata ownership semantics remain. 2026-09-14: the KeyTable pool is removed — the boot table is carved kernel-privately by Kickstart, runtime `Untyped.Retype` creates further tables from an Untyped's unused watermark range, and carved-table resolution uses address-guarded `resolve_carved` forms with alias rejection.)

- [ ] Establish Untyped-backed pool ownership, alignment, capacity, zero-sized-type policy, and charged metadata requirements; enforce unused-watermark/non-overlap/no-outstanding-access allocation and kernel-private KeyTable initialization. Do not add an unmap phase to ordinary Retype. (2026-09-12: carve primitives and the boot Untyped allocation are established in Kickstart's `bootstrap.rs`; the boot Domain pool and a boot KeyTable pool are carved, and the boot Domain's table lives in that pool. Runtime kernel-private KeyTable backing plus full pool/metadata ownership semantics remain. 2026-09-14: runtime Retype carves KeyTables kernel-privately from an Untyped's unused watermark range with a transactional watermark-last commit and consecutive-carve non-overlap verification; pool/metadata ownership beyond carved KeyTables — retirement, reuse validation, zero-sized policy — remain.)
- [x] Replace lifetime-erasing constructors and unrestricted object casts with access through the approved owning/locking context. (2026-09-07: `ObjectRef` removed; `KeyEntry` stores `ObjectId` only; `Access` context hands out borrow-checked guards.)
- [x] Implement allocated/incarnation/retirement checks before object dereference. (2026-09-07: per-pool `SlotMeta` state + generation validated before address computation; `Retired` represented, transition pending.)
- [x] Enforce unique kernel type mappings and handle aliased operands without fabricating exclusivity. (2026-09-07: unique `PoolTag` per pooled type; `resolve_pair_mut`/`validate_pair` reject same-slot aliases before constructing references.)
- [x] Make table insertion/removal/mutation preserve occupancy and slot-identity invariants. Validated with eight QEMU storage cases plus active-dispatch/ABI tests; management syscalls and object lifetime remain separate work.
- [ ] Enforce the selected single-core/non-reentry boundary and ensure scheduling occurs after guards end. SMP synchronization is deferred; concurrent cross-core access is not supported by this foundation.

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

**Interim Activate selection (2026-09-15):** the active `Domain.Activate` is the translation-context installation step only — `MAP` on the invoked Domain capability (mapping-context authority, consistent with root installation and ASID binding), a bound root and ASID, and the current Domain only. The execution-budget requirement lands with full Activate/Suspend/Resume (Phase 7, D8); the DCB-update sketch in `Nucleus::activate_domain` records that intent without being enabled.

**Recommendations:**

- Full table management is stronger than accepting a capability into a reserved receive slot. Keep destination authority explicit; narrower installation windows are a possible later refinement.
- Badges are authority metadata, not arbitrary caller identity. Define who may mint/rebadge a sender view; ordinary delegation must not silently allow impersonating another authority-bearing view.
- Copy is per-kind: Untyped cannot duplicate independent allocation watermarks; Reply cannot create independently usable replies; Time must conserve budget. **Latest confirmed Frame rule:** checked Copy duplicates capability authority, not a live mapping association; it is not Map. B maps the same physical frame through a separate Map operation, potentially at a different virtual page/Domain. This supersedes the earlier interpretation that Copy might install a mapping. A move preserves the binding needed for teardown.
- Validate and reserve a destination before removing source authority. Resolve aliases before constructing references or mutating operands.

**CONFIRMED clarification (2026-09-06):** use the general capability authorization rule: appropriate permissions on the source/destination KeyTable capabilities authorize table management, with no special manager-identity cases. Kickstart establishes the first Untypeds covering available memory and initial tables for predefined Domains, then delegates onward; exact tables/slots and handoff records remain open. **Selected follow-up (4A=A, 4B=B, 4C=A, 4D=A):** source/target entry selectors include expected incarnation; table permissions separately authorize source derivation, source removal/move, destination installation and deletion. CopyDerive/Move require vacant destinations and return destination-local keys. Move to the same table/slot is rejected, not a no-op; different slots in one table remain supported. CopyDerive preserves badges and attenuates rights; Move preserves rights/per-capability state. Delete removes an entry without automatic object retirement and permits cleanup after object retirement when the slot incarnation and table-management authority still match. Pre-commit failure preserves authority/accounting. The initial target-kind allowlist is KeyTable and existing-debug-gated DebugConsole, not arbitrary kinds. Exact rights bits and wire packing remain open; the canonical contract records the logical operand/result schemas. Selective Revoke, mapping teardown, and Reply/Time-specific transitions remain deferred; this does not reject their intended functionality.

**OPEN / DECISION REQUIRED — D4, plus D2/D3/D6/D8 where applicable:**

- [ ] Define per-kind semantic rights and the operation-to-rights matrix before freezing bit assignments.
- [ ] Define permitted attenuation of rights, extents, badges, budgets, and other per-kind authority.
- [ ] Set badge width, zero-badge meaning, and mint/rebadge/notification-bit policies.
- [x] Assign the selected separate table-management permissions and exact operation-to-rights combinations (2026-09-07): KeyTable rights bits are `DERIVE` (0x1), `REMOVE` (0x2), `INSTALL` (0x4), bit 3 reserved. CopyDerive = source `DERIVE` + destination `INSTALL`; Move = source `DERIVE` + `REMOVE` + destination `INSTALL`; Delete = `REMOVE`. Enforcement in the kernel handler remains implementation work; narrower receive-slot windows remain a later refinement.
- [ ] Specify Kickstart's initial Domain/table capacities, slots/grants and incarnation-bearing handoff, keeping boot reservations/accounting correct and real KeyTable capabilities distinct from manager-service endpoints; settle Domain.Grant's relationship to KeyTable operations.
- [x] Implement the selected KeyTable/debug-console CopyDerive/Move/Delete subset and transactional failures; settle other per-kind semantics before extending the allowlist. (2026-09-07: handler active for the caller's own table via CAPTBL_SELF with atomic validate-commit and Move rollback; cross-table resolution and retired-object cleanup await pooled KeyTables and the retirement transition. 2026-09-14: cross-table resolution is active on carved tables through `resolve_carved` guards; retired-object cleanup still awaits the retirement transition.)

### Who tracks revocation?

**CONFIRMED authority split (2026-09-06):** userspace components granted management capabilities/rights may derive, install, and manage capabilities directly. They are also entrusted with the associated bookkeeping and form part of the TCB of that libOS composition, within their granted authority. There may be multiple managers or a hierarchy; that is policy, not a kernel-mandated topology. KeyMaster names a management role, not a unique kernel-recognized service.

A Key selects a slot/incarnation; KeyTable capabilities provide authority for table operations. The existing Vesper abstraction already expresses the distinction between using an object and managing entries that name it. A component without direct management authority can call a manager service instead. A component explicitly granted direct derivation power is not required by the nucleus to register afterward with a central KeyMaster; its bookkeeping obligations belong to the entrusted composition.

The kernel supplies checked source/destination authorization, per-kind transitions, rights attenuation, lifetime validity, and quiescence/reuse mechanisms. Composition-scoped TCB membership does not grant unconfined execution or bypass kernel memory protection. This is the selected **SPeCK-like resource-management model**, not wholesale adoption of Composite's layouts or every implementation detail. The table below preserves historical alternatives; kernel-owned tree management and a mandatory kernel ancestry chain are not the chosen direction.

| Model | Benefits | Costs |
|---|---|---|
| Composition chooses one entrusted manager | Smaller kernel derivation machinery; clients use its services | Direct management authority must be granted consistently with that policy; leaks/recovery remain OS concerns |
| Kernel tracks derivation scopes | Supports direct delegation between mutually distrustful components | Charged metadata, traversal, bounded cancellation and reclamation |
| Coarse allocation/delegation scopes | Simpler initial revocation mechanism | Broader disruption; less selective reclamation |

**Selected split:** KeyMaster tracks the tree; derived caps across tables reference a common object identity. Kernel object retirement changes that object's validity/generation, rejecting all previous cap invocations. KeyMaster then cleans up dead entries and derivation metadata in the background. Shallow trees are a performance expectation, not a requirement to traverse ancestry on every invocation.

An object-wide epoch does not provide selective branch-only invalidation while sibling caps to the same object remain valid. If that separate operation is required, KeyMaster must use an explicit invalidation protocol; the exact primitive/completion boundary remains open. Do not add kernel ancestry metadata as a presumed prerequisite for the already-selected object-retirement mechanism.

Lazy object invalidation does not remove PTEs or cancel all retained work by itself. **The authorized library OS/resource manager owns unmap-before-invalidation orchestration.** Trust follows explicitly granted capabilities, not a blanket assumption that all Domains or libOS instances cooperate. It must not invalidate the only usable cleanup authority and then expect a malicious recipient to cooperate. Premature-reuse checks and the kernel/manager completion protocol remain D2/D6, but the new D1 threat model requires confinement even when a recipient bypasses its libOS. Accepted allocation leaks must not become conflicting physical reuse.

The earlier question about applications deriving directly is resolved: granting direct management authority also entrusts the recipient with bookkeeping. Multiple or hierarchical managers distribute this responsibility through composition policy. No universal central registration protocol or name-based manager privilege is implied. Exact manager-side synchronization, partial-failure handling, and coordination remain work for the chosen composition, not a new kernel policy mandate.

**OPEN / DECISION REQUIRED — D2:**

- [ ] Specify/enforce management authority for each derivation/installation/transfer path, including source/destination table checks and per-kind attenuation; do not impose central post-hoc registration.
- [ ] Define manager-side bookkeeping contracts for the selected libOS composition, including failure handling and any cross-manager/hierarchical coordination; granting direct derivation power entrusts that responsibility.
- [ ] Implement shared-object validity checks across tables, and define stable authoritative metadata for inline Frame/Untyped regions as well as pooled objects.
- [ ] Define the kernel-retirement/KeyMaster-cleanup notification or handoff, idempotent cleanup, and generation-safe metadata reuse.
- [ ] Define selective subtree invalidation of a still-live shared object if required; do not assume the object-wide epoch preserves siblings.
- [ ] Define which mappings and pending operations belong to a revoked scope, including derivations/transfers racing with revoke.
- [ ] Implement invalidation and reclamation completion only after the selected scope and authority contracts are approved.

## 5. Direct userspace memory access is not a fallible invocation

### Current code and limitation

Included [`DcbView`](../libs/object/src/domain.rs) constructs references into shared mapped memory. The [`Buffer` wrapper](../userspace/buffer/buffer.rs), intended functionality currently excluded from compilation, creates ordinary slices; it moved out of `libs/object` when Buffer was selected as a userspace/libOS construct (2026-09-15).

“Direct references” means `&[u8]`, `&mut [u8]`, or `&DomainControlBlock` produced by these userspace wrappers after mapping—not raw pointers granting authority across the kernel boundary. The control interface remains capability-based; subsequent loads/stores are not capability invocations and cannot return the inconsistency status.

`MappedSlice` and `MappedSliceMut` already invoke Unmap on Drop. **Intended ownership design:** create and own a private capability/mapping inside the guard, allow no further derivations or independently usable management aliases, and release the mapping on Drop. The current sketch does not yet implement this: `map_slice[_mut](&self)` borrows an existing BufferKey and stores that borrow rather than creating/owning a dedicated capability.

**CONFIRMED cleanup guarantee:** Drop uses an incarnation-bearing capability key and the necessary kernel checks. A stale key is rejected rather than allowed to unmap a replacement. This follows the general key contract, not a new destructor exception. Tests must establish it once the new keys exist; the current destructor discards errors. The private mapping design prevents another holder from independently remapping that same guard-owned association; its lifecycle must enforce this, rather than treating a normal remap as capability invalidation. A forgotten guard may leak, which is acceptable if it cannot cause invalid reuse.

There are two different exclusivity questions: ownership of the capability/mapping, and access to the physical bytes. No derivations from the guard-owned capability addresses the first. It does not prove that the physical frame has no aliases in other Domains, ancestor retirement authority, or device writers. D1 now prohibits multiple virtual aliases within one Domain, but permits cross-Domain writable sharing.

**CONFIRMED priority:** authorized revocation is the ultimate source of access validity and is not vetoed by a client holding a Rust borrow. Higher-level protocols coordinate safe use. Access after completed withdrawal to a still-unmapped address faults; the expected policy is likely termination of the offending Domain, with exact fault/supervisor semantics still open. Private mapping ownership does not prevent ancestor revocation. Revocation must retire hardware access, not just capability lookup, to supply this behavior.

**Preferred Rust direction:** explicit unsafe caller obligations for reference-producing access, because the kernel cannot generally promise cross-Domain exclusivity of shared resources. This is not a decision to make every mapping operation unsafe. The exact unsafe API and its full-borrow obligations remain to be designed; raw/fbuf protocol access may be preferable where an ordinary reference cannot be justified. Safe wrappers may exist only where they actually enforce stronger conditions. The kernel must remain memory-safe and isolate unrelated authority even if the application violates its unsafe contract.

**Technical correction — read-only is not immutable backing:** read-only PTE permissions prevent stores through that particular mapping. For example, A can map frame F read-only while B maps F read-write; B's permitted write changes the bytes A observes. DMA and kernel writers can also change them. This is consistent with hardware read-only protection, but inconsistent with calling the backing immutable. A read-only DCB mapping updated by the kernel is an existing example. Ordinary `&[u8]` needs no conflicting mutation for its borrow, not merely an inability to write through A's PTE. An immutable-sharing mode must establish a stronger backing/protocol guarantee.

**DEFERRED — stale raw pointers after VA reuse:** a capability generation is checked on invocation, not an ordinary load. If revoked address X is later mapped to accessible replacement backing, an old raw pointer X may access that replacement instead of faulting. The maintainer explicitly does not require X to remain inaccessible for the surviving Domain's lifetime: outside/higher-level mechanisms must prevent stale application accesses for now. Stronger temporal-address guarantees/quarantine are a far-future topic, not a current kernel implementation prerequisite. Frame-cap slot non-reuse is not virtual-address non-reuse. This deferral does not weaken kernel lifetime safety, generation checks, completed mapping withdrawal/TLB synchronization, or safe physical-resource reuse; it also does not make an invalid Rust reference sound.

### What an ordinary `&[u8]` requires

The standard library's [`slice::from_raw_parts` safety contract](https://doc.rust-lang.org/std/slice/fn.from_raw_parts.html#safety) is the relevant boundary when a mapping wrapper creates a slice. For `&'a [u8]` it requires:

1. A non-null pointer, correctly aligned, valid to read all `len` bytes. Non-null/alignment still apply to empty slices; `u8` alignment is one byte.
2. Consecutive initialized bytes in a single valid allocation, with correct bounds/provenance. Every initialized bit pattern is valid for `u8`, but uninitialized memory is not. Adjacent mappings/allocations are not automatically one Rust allocation. Physical contiguity is not required; the mapping abstraction must establish the virtual allocation it exposes.
3. Total byte length at most `isize::MAX`, without address arithmetic wrapping.
4. Memory that remains valid for the entire borrow: no deallocation, unmapping, or replacement that invalidates the reference while it is live. The wrapper must tie the slice lifetime to the actual access contract, not manufacture permanence.
5. No mutation of those bytes for the borrow's duration. The documented interior-mutability exception concerns `UnsafeCell`; ordinary `u8` elements provide no such exception. Another Domain's writer, DMA, or the kernel can violate this even if the observer's PTE is read-only. Multiple shared readers are allowed, but competing exclusive/mutating access is not.

Wrapping backing in `UnsafeCell` does not permit returning an ordinary `&[u8]` and then mutating through other paths during that borrow. Expose a suitable cell/atomic/protocol interface, or establish a non-mutating access interval before creating the ordinary slice. `UnsafeCell` itself is not synchronization. An unsafe constructor assigns these obligations to the caller; it does not switch off Rust's reference rules.

A possible fbuf read-phase protocol is: producer initializes/publishes bytes with appropriate ordering → reader obtains an access interval during which the payload stays valid and no participant modifies it → reader drops all ordinary slice borrows and releases/acknowledges the interval → producer may reuse/mutate the payload. Protocol state can use appropriate atomics separately from the borrowed payload. This is an implementation idea, not an approved fbuf wire/state machine; a peer that can ignore the protocol prevents an unconditional safe-reference promise. Lifetime and revocation coordination remain outside mechanisms under the selected unsafe direction.

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
| Coordinated access interval/pin | May support a stronger wrapper if compatible with the protocol, but must not give an uncooperative recipient a veto over authoritative revocation; pins alone do not establish exclusivity |
| Copied, fallible observations | Avoid exposed persistent references; may require an agreed query operation and syscall cost |
| Shared-memory-specific access protocols | Appropriate for atomics, producer/consumer data, DMA, or MMIO; not automatically ordinary Rust slices |

**OPEN / DECISION REQUIRED — D1/D3/D5/D6/D7:** implement private guard-owned mappings and specify the preferred unsafe/fbuf protocols under authoritative revocation. All three sharing modes are required: exclusive ownership, immutable sharing, and synchronized mutable sharing. Persistent DCB views and ordinary revocable buffers have different lifecycles; view persistence does not imply immutable contents.

- [ ] Implement dedicated guard-owned capability/mapping creation and teardown with no derivation or management-handle escape; replace the existing shared BufferKey borrow.
- [ ] Validate stale Drop rejection through the same incarnation checks as other invocations, and prevent independent remap of a live guard-owned association.
- [ ] Define unsafe full-borrow validity obligations and fbuf coordination under ancestor revocation/object retirement without making client borrows veto revocation.
- [ ] Select the reference-producing unsafe boundary and stronger safe wrappers, if any; state what physical-byte validity, aliasing, and writer guarantees each requires.
- [ ] Define exclusive/immutable-shared/mutable-shared mode transitions and synchronization independently of per-mapping permissions, including writers in other Domains and DMA.
- [ ] Define fault delivery and the likely Domain-termination policy after revoked access; distinguish capability inconsistency errors from CPU protection faults.
- [ ] **Far-future, deferred:** revisit temporal-VA reuse/quarantine or stronger stale-pointer detection only in a separately scoped design. For now outside mechanisms prevent stale application accesses; this is not a dependency of the current capability/mapping refactor.
- [ ] Define buffer lifetime and input stability across blocked operations, including copying, pinning, or revalidation where appropriate.
- [ ] Keep arbitrary safe slice construction unavailable until validity, aliasing, and mutation guarantees are enforceable.

## 6. Domain identity and DCB observation

### Current code

[`DcbPages`](../kernel/nucleus/src/objects/domain.rs) release indexes allocation metadata before validating the full ID range. Lookup checks page availability, not allocated incarnation. Reuse has no generation protection.

[`Nucleus::current_domain_mut` and `current_dcb_mut`](../kernel/nucleus/src/objects/nucleus.rs) now reject absent current-domain identity rather than falling back to domain zero. Active dispatch returns the existing `InvalidDomain` error. The boot fixture explicitly selects its first private domain after creation; exception-origin/caller binding remains unfinished. Domain-pool allocation, table ownership, and DCB allocation are not yet one coherent lifecycle, and this repair does not add incarnation-safe reuse or DCB allocation/publication checks.

The shared DCB has unresolved layout/stride and publication issues, including non-atomic identity fields that cannot be rewritten concurrently with readers under an unsupported reference model.

**Recommendation:** allocate private domain state, DCB identity, and table ownership coherently; reject absent/stale caller identity. Before domain reuse, retire waits, replies, donations, scheduler relationships, and other retained references.

**CONFIRMED direction:** DcbView is persistent and does not disappear like an ordinary buffer mapping. Record this rather than treating arbitrary DCB unmapping as a design requirement. The remaining questions are backing availability for each page, domain-record incarnation/reuse, publication, and snapshot consistency. A persistent view does not mean the observed domain lives forever. A version check alone does not legalize racing non-atomic Rust reads. Prefer sound value/snapshot accessors over unrestricted references where necessary.

**OPEN / DECISION REQUIRED — D3/D5, with D1/D7/D8 dependencies:**

- [ ] Choose DCB layout/stride/capacity, visibility, discovery, mapping lifetime, and identity-reuse protocol.
- [ ] Specify independently observable fields versus coherent snapshots and a Rust-sound publication mechanism.
- [ ] Define legal Activate/Suspend/Resume transitions, preserving blocked continuations and execution-budget requirements.
- [ ] Implement coherent domain allocation/retirement and bounds/allocation/incarnation checks.
- [x] Reject absent caller identity instead of implicitly selecting domain zero. Validated by the six-case `just test-debug-console` QEMU harness (three new caller-context regressions), existing host ABI/client tests, full `just clippy`, `just fmt-check`, the debug-enabled coordinated build, and `just test-device`; see the plan's absent-caller rejection slice for coverage limits. Exception-origin binding and domain lifecycle remain separate work.

## 7. Pending operations, replies, and Time

### Current code and reachability

The Endpoint, Reply, and Time operation/object files are excluded because they do not compile as-is today. Notification left this set on 2026-09-16 and EventCount left it on 2026-09-18: both are Retype-creatable and active through real SVC dispatch, including the blocked-wait park/resume entry path (validated end-to-end 2026-09-18, with EventCount's broadcast wakeups and overflow error-completions exercising the same foundation). The remaining excluded files stay intended functionality and must participate in lifecycle design now. Their current code is not evidence of supported syscall behavior; exclusion is not a reason to discard their design or omit future implementation.

- [`Endpoint`](../kernel/nucleus/src/objects/endpoint.rs) queues multiple callers but stores one endpoint-global pending message. Its two arrival orders do not implement equivalent reply/completion behavior.
- [`Reply`](../kernel/nucleus/src/objects/reply.rs) identifies a caller rather than a particular invocation and removes transferred authority before fallible destination installation.
- The excluded [`Reply` userspace wrapper](../libs/object/src/reply.rs) forgets ownership before checking success. [`Time`](../libs/object/src/time.rs) uses slot deletion in `Drop`; consuming wrappers do not yet have approved loan/transfer/failure semantics.

For a blocked caller, “the next invocation fails” is inadequate: there may never be another invocation. Retirement must arrange an explicit continuation outcome. Revocation/timeout cannot undo already committed effects or imply that a delivered request was never processed.

**CONFIRMED (2026-09-16, D7):** a bounded pending-invocation record owns payload, badge/authority metadata, phase, reservations, and eventual result. Both rendezvous arrival orders use one commit transition. Reply names that invocation; reply, timeout, cancellation, and teardown compete for exactly one terminal transition. The wait/timeout/cancellation semantics are selected in the canonical contract (one relative nanosecond timeout per blocking operation, phase-specific Call outcomes, late replies consumed with a defined replier error, cancellation by timeout/teardown only, one-consumer notification delivery).

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
- [x] Define per-phase timeout/cancellation semantics, late replies, open/closed wait identities, and terminal results. (2026-09-16: defined in the canonical contract's selected wait/timeout/cancellation semantics. Implementation status: the infinite-wait blocking path and its terminal delivery are active for `Notification.Wait` (2026-09-18); timeouts, late replies, and the remaining primitives remain open.)
- [ ] Decide whether already-delivered calls remain replyable after endpoint retirement.
- [ ] Define explicit deferred completion and bounded wait/reply reservation, including domain/capability/resource teardown. (2026-09-18: explicit deferred completion and bounded reservations are implemented and validated end-to-end for `Notification.Wait` and `EventCount.Await` — pending-pool capacity, per-object wait-queue capacity with before-admission rejection, and the park/resume entry path, with completed records now carrying the full result shape so error wakeups (an overflowing advance) resume with their status. 2026-09-19: domain-teardown-driven cancellation of all pending records is implemented — `Nucleus::cancel_domain_pending` removes the torn-down Domain's records from every wait queue, gives each its single terminal transition, releases it (a torn-down waiter never resumes), and purges any queued wakeup; validated by model tests and the boot fixture's teardown of a parked Domain, with no cancellation status crossing the wire. Later the same day: the teardown trigger is active as `Domain.Retire` (op 4, `RETIRE` right, selected 2026-09-19) through real SVC dispatch. The item stays open for reply/resource-teardown reservation, which lands with Endpoint/Reply.)
- [ ] Preserve source ownership on pre-commit failure; return it from consuming client APIs when retry remains possible.
- [ ] Represent irreversible partial completion explicitly, especially reply-committed/receive-failed.
- [ ] Make destructors best-effort fallback, not the sole guarantee of releasing callers or recovering budget; cleanup must not target replacement slot incarnations.
- [ ] Choose Time donation as loan versus transfer, unused-budget destination, provenance, deletion/expiry/cancellation outcomes, and multicore accounting.
- [ ] Implement one-shot reply consumption, race-free notification/event-count waits, and budget conservation on the shared completion foundation. (2026-09-18: race-free notification and event-count waits are implemented on the foundation — validate-before-reserve registration, one-consumer notification delivery, broadcast event-count wakeups with overflow error-completions, enforced single terminal transition, park/resume; one-shot reply consumption and budget conservation remain.)

## 8. Memory reclamation and protection

### Current code

The excluded [`Frame` handler](../kernel/nucleus/src/api/arch/frame.rs) records mapping state without implementing the hardware transition. Included inline region mapping state in [`KeyEntry`](../kernel/nucleus/src/api/key_entry.rs) lacks enough context for general teardown.

The excluded [`Untyped` retype sketch](../kernel/nucleus/src/objects/untyped.rs) advances its watermark before subsequent fallible creation/insertion. The excluded Buffer handler (which could lose bookkeeping around partial map/unmap failures) was removed on 2026-09-15 when Buffer was selected as a userspace/libOS construct; the kernel maps Frames only.

**Recommendation:** durable mapping identity, transactional creation, and retained partial-teardown bookkeeping. Before reuse, complete required CPU TLB/device translation synchronization and sanitize fresh ordinary RAM crossing protection boundaries. Intentional content-preserving sharing and device memory require distinct treatment.

The shared-address-space protection decision matters here: capability checks on syscalls cannot mediate arbitrary CPU loads/stores through installed translations. A common address namespace also does not imply that only one CPU can cache a translation.

### CONFIRMED Frame mapping direction

- **Copy is not Map.** Checked Copy creates another permitted capability to the same physical frame, without duplicating a live mapping association or installing a PTE. Separate Map creates that cap's mapping.
- The same physical frame may be mapped at different virtual pages in different Domains through capabilities derived from the same origin; the same virtual address is preferred for fbufs and pointer sharing. A frame must not be mapped at two distinct virtual addresses within one Domain. This must cover physical overlap through different caps or large/small frame aliases, not just duplicate handles. Simultaneous writable mappings across Domains are permitted with higher-level synchronization.
- Unmap is mapping-local, whether invoked through origin A or derived B; it does not itself traverse descendants.
- **Terminology correction:** the maintainer's earlier “origin Unmap” meant capability **Revoke on the origin**, as in seL4, not a stronger Frame.Unmap. Origin Revoke withdraws descendants while retaining the origin; removing the origin's own mapping/capability is separate. Earlier text describing an Unmap primitive that removes A+B was a misunderstanding and is superseded. KeyMaster implements tree policy using kernel mechanisms; object retirement is still distinct.
- Remapping is origin-authorized and remains a valid operation on the capability. The concrete effect on descendant mappings, and virtual relocation versus replacing physical backing, remain open.
- For now, deprovisioning requires full revocation and no reuse of that slot for Frames; scope and capacity consequences need definition.
- The libOS must perform the correct Unmap/retirement order. Kernel object invalidation rejects cap invocations; it is not automatic PTE teardown. KeyMaster's background cleanup is a separate responsibility.

The current inline `RegionPayload` has physical address and compressed per-cap mapping state, but no shared Frame lifetime generation or full target-context identity. Add the authoritative frame/allocation lifetime and mapping metadata needed for correctness, freely changing object/key sizes or introducing shared records. Keeping extent metadata inline is optional, not a constraint. Audit size-dependent calculations and views whenever these layouts change, then optimize in a later measured pass.

**OPEN / DECISION REQUIRED — D1/D2/D4/D6:**

- [ ] Implement backends for the selected hostile-code threat model and Domain = VSpace protection boundary; resolve per-target features without weakening confinement.
- [ ] Define physical and metadata layouts, accounting, single/batch retype, and initialization/sanitization enforcement.
- [ ] Define complete per-mapping identity, permissions/attributes, ASID ownership, and hardware-safe namespace reuse. (2026-09-15: mapping-context identity selected — the Domain is the mapping context; there is no separately targetable VSpace kernel object, and the registered VSpace ID stays reserved. ASID ownership selected — explicit capability-protected ASIDPool/ASID resources with authorized AssignASID-style binding to a Domain's translation root. Later the same day: ASID binding implemented — boot-provided pool with bitmap allocation (ASID 0 reserved for the kernel's boot context), `ASIDPool.Assign` wire schema with `GRANT`/`MAP` authority and failure atomicity, the bound ASID recorded on the Domain, and ASID-scoped TLB invalidation on unmap. Complete per-mapping record contents, permissions/attributes, ASID release/hardware-safe namespace reuse, and multi-pool partitioning remain open.)
- [ ] Specify/implement separate Copy and Map schemas: Copy installs only the destination capability; Map validates target context, address, rights, and mapping state before installing a PTE.
- [ ] Specify origin-capability Revoke orchestration, descendant mapping withdrawal, failure/partial completion, and prevention of racing remaps/derivations; keep local Unmap and origin removal distinct.
- [ ] Specify virtual remap versus physical-backing replacement and whether derived mappings stay put, follow, or are withdrawn; origin permission alone is not a userspace pointer-lifetime proof.
- [ ] Encode/enforce origin-only remap independently of delegable lifetime-control permission; a derived retirement-authorized capability does not thereby gain remap permission.
- [ ] Define libOS Unmap-before-invalidation prerequisites and the kernel checks/trusted-manager obligations that prevent premature reuse.
- [ ] Implement validate → reserve → initialize/prepare → commit for retype, mapping, and transfer, with rollback or explicit recoverable partial state.
- [ ] Retain teardown bookkeeping until mapping/device invalidation completes; do not reset an allocation watermark as a substitute for revoke.
- [ ] Implement backing reuse only after authority retirement, pending-use retirement, hardware synchronization, and required sanitization.

### D1: selected protection and sharing architecture

Maintainer answers to the D1 questionnaire establish the following direction. These are approved semantics, not implementation/test results.

| Topic | Selected direction |
|---|---|
| Adversary | Arbitrary native code, including actively malicious code and deliberate confinement attacks |
| Trust | Explicit capability scope; KeyMaster is entrusted with key/tree operations because it holds appropriate authority, not because of a kernel name-based exemption |
| Protection unit | Domain and VSpace are the same protection-context boundary; separate Domains are independent |
| Execution privilege | Everything except the nucleus is userspace by default; no unconfined service promotions initially |
| Required confidentiality/integrity | Other Domains' memory absent granted access, and kernel-private memory always |
| Single address space | Shared numerical meaning when correctly mapped, plus cheap sharing; a single root/no-switch execution is not required |
| Preferred IPC payload path | fbufs mapped at matching addresses in peers, with pointer sharing and higher-level synchronization |
| Intra-Domain frame aliases | Prohibited: one physical frame must not have two distinct virtual addresses in one Domain |
| Cross-Domain frame aliases | Different addresses permitted; matching addresses preferred |
| Cross-Domain writers | Permitted; fbuf protocols coordinate shared mutation |
| Sharing modes | Exclusive, immutable-shared, and mutable-shared all required |
| DMA | Trusted mediation or IOMMU confinement, depending on the platform |
| Availability | libOS policy; nucleus provides resource isolation/abstraction and IPC mechanisms |
| Timing/cache side channels | Deferred, but tracked in design; not part of the current confidentiality guarantee |
| Revocation | Authoritative over client references; subsequent access to withdrawn/unmapped memory faults, with Domain termination the likely policy |
| Rust shared-memory access | Preference for explicit unsafe caller obligations; exact reference/fbuf APIs remain open |
| Hardware range | PowerPC G5 through current Intel, Armv9, RISC-V; potentially higher-end STM32 |
| Fallback | Separate protected translation contexts, preserving shared-address conventions where possible |

SPeCK-like userspace policy over kernel liveness/resource/quiescence mechanisms remains the selected architecture. A Domain's possession of a permission authorizes its specified operation; it does not imply that its arbitrary syscall inputs are safe, that it follows fbuf protocols, or that it may violate protection of unrelated objects. Applications can deliberately damage resources they are permitted to write/retire; such delegated power is not a confinement escape.

Domain = VSpace is a semantic protection-unit decision, not permission to silently merge the current public type IDs or cast DomainId into an ASID. Internal object representation, execution-context relationships, and backend identity binding still require a coordinated design.

### D1 clarifications and consistency checks

**1. Read-only mapping versus immutable backing — technical correction.** The inability of A to store through its read-only PTE does not prevent B's writable PTE from changing the same frame. The read-only DCB view is also intentionally updated by the kernel. Consequently, immutable sharing requires more than read-only permissions at one observer. The record preserves the hardware write-protection requirement without adopting the incorrect inference of global immutability.

**2. CONFIRMED — fbuf address agreement precedes mapping.** Fbuf setup establishes addresses suitable for all two-or-more participating Domains before installing mappings, and publishes pointers only after successful setup. F at X in A and Y in B is otherwise allowed, but passing pointer X to B does not work merely because B maps F at Y. The no-intra-Domain-alias rule means B cannot keep Y and additionally map F at X. Address negotiation/reservation must resolve that conflict rather than silently relocate escaped pointers. Every shared pointer's pointee also needs the intended authorized mapping; mapping a pointer-containing buffer does not grant access to arbitrary targets.

**Scope decision:** multi-node global/distributed address allocation is out of scope. Do not import a classical SASOS assumption that one coordinated 64-bit namespace covers local RAM, neighboring nodes' RAM, and allocated disk space. The maintainer has raised concerns about global reservation; the exact machine-local reservation/allocation model remains open rather than being silently replaced by a specific scheme. The current concrete requirement is participant-compatible fbuf addresses established before mapping.

**3. DEFERRED — stronger raw-pointer temporal guarantees.** Completed hardware withdrawal can make stale accesses fault while addresses remain unmapped. Once X is legitimately reused for accessible memory, the old pointer X may access the replacement without a capability-generation check. The maintainer explicitly declines a lifetime-long inaccessible-address requirement: outside mechanisms must ensure no stale application accesses for now. Preserve stronger temporal-VA mechanisms as far-future research, not a current blocker. This is distinct from Frame-cap slot non-reuse and from mandatory present-day hardware withdrawal, remote TLB/in-flight synchronization, and physical reuse safety.

**4. Unsafe Rust versus fault containment.** Unsafe shared-memory APIs can place validity, exclusivity, and synchronization obligations on their callers. They cannot promise ordinary Rust reference soundness if those obligations are violated; an eventual protection fault does not repair undefined behavior or compiler assumptions. That is an application/protocol error, but the nucleus must remain memory-safe and preserve confinement from malicious native code. Fbuf APIs need not expose ordinary references when the full-borrow guarantee is unavailable.

**5. Hardware breadth — features are not uniform.** Separate translation contexts are an acceptable fallback, but MPU-only targets may isolate regions without supporting arbitrary virtual aliases or page-table-style remapping. Per-target support restrictions and whether a requested mapping mode is unsupported must be explicit. Software-mediated DMA requires exclusive control of programming paths/descriptors; giving an untrusted Domain raw device-programming authority can bypass mediation. This document does not claim all listed machines currently support the same features.

**6. Availability policy versus resource enforcement.** Keeping recovery/admission policy in the libOS is consistent with the model. A malicious Domain may bypass its libOS, so the nucleus still must bound/charge its resource consumption and provide checked IPC/syscall failure rather than unbounded allocations or panics. Exact budget/quota and bounded-work mechanisms remain implementation work, not a new kernel scheduling-policy mandate.

**7. CONFIRMED — direct derivation entails bookkeeping responsibility.** Applications granted explicit copying/derivation-management authority join the TCB of the particular libOS composition and are responsible for bookkeeping. Multiple managers or hierarchies are composition policy. The kernel provides mechanisms and checks authority; it does not impose a central registry or special KeyMaster identity. The remaining work is implementing operation schemas and composition-specific management protocols, not choosing between direct derivation and mandatory registration again.

### D1 implementation and validation work

- [ ] Implement fbuf address agreement for all participants before mapping; define reservation/conflict detection, stability during installation, and pre-publication failure cleanup. Keep the machine-local allocator choice explicit and multi-node global address allocation out of scope.
- [x] Implement no-intra-Domain-alias checks across physical overlaps, including distinct caps and large/small frame extents; define who enforces them so arbitrary native code cannot bypass the selected rule. (2026-09-15: the kernel enforces the rule in the `Frame.Map` handler — the only mapping-installation path, unreachable without a capability invocation — ahead of the hardware transition: the arch layer walks the target Domain's installed tables and rejects a candidate whose physical extent overlaps any live page/block descriptor with `PhysicalAlias` (status 30, detail 1 the conflicting physical base). The interval check covers distinct derived caps and mixed frame sizes by construction; cross-Domain aliases remain distinct PTEs. Boot/host tests cover the derived-cap second-vaddr rejection and the post-unmap success; see the alias-policy enforcement slice in the implementation plan. Mixed-size overlap and cross-Domain success are covered by construction but not yet exercised — Retype carves disjoint extents and only the boot Domain exists.)
- [ ] Bind each Domain to its independently protected backend context while reconciling existing Domain/VSpace ABI/storage responsibilities without renumbering by accident.
- [ ] Define a target MMU/MPU/IOMMU, address-width, page/region-granularity, and DMA feature matrix; document unsupported modes rather than downgrade malicious-code confinement silently.
- [ ] Specify kernel/userspace transitions and caller identity; remove bootstrap-only EL1/domain-zero assumptions before claiming user confinement.
- [ ] Define revocation completion, fault delivery and likely Domain termination for access to withdrawn mappings. Do not require permanent inaccessible VAs or solve stale raw-pointer reuse in this slice; outside mechanisms own that prevention for now.
- [ ] Implement fbuf sharing modes, synchronization, and preferred unsafe access contracts; distinguish read-only views from immutable backing.
- [ ] Specify resource-accounting/bounded-work mechanisms supporting libOS policy against callers that bypass library wrappers.
- [ ] Plan adversarial agent tasks and regression tests for unauthorized loads/stores, privileged operations, malformed invocations, cross-Domain/key-table attacks, alias-rule bypass, DMA bypass, revocation races, and reuse.
- [ ] Keep timing/cache side-channel exposure recorded as deferred rather than claiming it is solved by address-space isolation.

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

#### Recent Composite activity and newer research

Public sources checked on 2026-09-06: the old SPeCK/PARSEC/C3 papers do not imply abandonment. The [official repository](https://github.com/gwsystems/composite) is not archived, and the already-inspected source pin [`3ef8f8c4d3296624640e6f3bd00801054d8350a3`](https://github.com/gwsystems/composite/commit/3ef8f8c4d3296624640e6f3bd00801054d8350a3) is a substantive merge dated **2026-08-31**. [PR #502](https://github.com/gwsystems/composite/pull/502) integrates generic guest-image builds, VMM I/O changes, and a networking-thread placement fix. [PR #498](https://github.com/gwsystems/composite/pull/498), merged **2026-02-18**, integrates Patina and VMX updates. These establish recent research development, not production support.

Newer directly relevant primary work includes:

| Work | Date | Relevance and limit |
|---|---|---|
| [Ch'i: Scaling Microkernel Capabilities in Cache-Incoherent Systems](https://faculty.cs.gwu.edu/gparmer/publications/chi20ross.pdf) | ROSS 2020 | Composite-based capability visibility/quiescence and safe reuse on incoherent systems; not a general derivation-tree policy |
| [Practical Principle of Least Privilege for Secure Embedded Systems (Patina)](https://faculty.cs.gwu.edu/gparmer/publications/rtas21patina.pdf) | RTAS 2021 | Section IV-C directly discusses user-level capability-manager delegation/revocation policy and statically bounded tracking specialized for restricted sharing patterns; not arbitrary-depth tree management |
| [Janus: OS Support for a Secure, Fast Control-Plane](https://faculty.cs.gwu.edu/gparmer/publications/rtas25janus.pdf) | RTAS 2025 | Composite capability-controlled fast paths using MPK; section III-E links capability revocation to removing fast-path callgate/dispatch access. Its threat model/backend cannot be assumed to satisfy Vesper D1 without review |
| Byways: High-Performance, Isolated Network Functions for Multi-Tenant Cloud Servers | SoCC 2024 | BywayOS is built as components on Composite; evidence of continued systems research, not a completed general revocation service |
| SPR: Shielded Processor Reservations with Bounded Management Overhead | RTAS 2025 | Composite implementation/evaluation of temporal-isolation mechanisms; not derivation bookkeeping |

The latter two and other newer work are listed in the author's [publication catalogue](https://faculty.cs.gwu.edu/gparmer/pubs.html). Bibliography/site maintenance can lag actual development.

**Maturity qualification:** the inspected repository still labels itself pre-alpha; public GitHub release/tag listings were empty at this check. Recent activity and papers establish that calling it abandoned is unsupported, but do not establish stable releases, long-term support, or completed failure-resilient revocation in current `capmgr/simple`. Patina is the most direct newer reference for the external-management question; the earlier source-level completeness caveats still apply.

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
- [ ] Enforce explicit management authority and implement the retirement/background-cleanup handoff for the composition's entrusted managers, without requiring a singleton KeyMaster or central post-hoc registration.
- [ ] Settle remaining selective-revocation and completion guarantees, including Unmap-before-invalidation and safe physical reuse.
- [ ] Implement private guard-owned mapping lifecycles, investigate Rust mutability/shared-resource APIs, and define persistent DCB record guarantees.
- [ ] Resolve remaining namespace scope, common-address reservation/conflicts, and relocation policy under the selected cross-Domain-alias/protected-context model; do not reopen the permitted aliasing modes.
- [ ] Keep remaining decision approvals, schemas, and implementation evidence synchronized with the canonical contract and plan.
- [ ] Implement guarded storage and the smallest approved KeyTable lifecycle first; leave unresolved revoke/derivation behavior unsupported.
- [ ] Follow with domain/DCB, memory reclamation, deferred completion/IPC, and Time slices according to their actual prerequisites, not by enabling existing sketches wholesale.

### Validation to carry out

Use the repository's documented Justfile workflows. Pure ABI/model tests do not substitute for target validation of mapping, hardware synchronization, or blocked-call resumption.

- [ ] Test stale slot/object/domain identities, same-type replacement, diagnostic inconsistency reasons, slot exhaustion with deletion still permitted, and generation/rebinding reuse safety.
- [ ] Test one object retirement rejecting old derived caps in multiple tables while background tree cleanup has not yet run; distinguish selective branch revocation.
- [ ] Test creator control surviving delegation, unauthorized retirement, and safe leaked allocations without automatic final-capability retirement.
- [ ] Test same-table/same-object aliases, same-slot Move rejection, occupied destinations, stale management selectors, deletion of entries naming retired objects, null insertion, capacity exhaustion, and bookkeeping consistency.
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
