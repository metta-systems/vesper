# Lifetime and authority semantics

## Status and relationship to the capability contract

Design investigation and maintainer follow-up. **This document distinguishes maintainer-confirmed semantics from research, proposals, and unanswered questions.** The confirmed subset is recorded in the canonical contract; neither documenting an alternative nor citing seL4/Composite adopts it as Vesper policy. No new wire encoding or runtime implementation is introduced here.

- [Nucleus capabilities](nucleus_capabilities.md) remains the canonical contract and decision register (D1–D9).
- [Capability implementation plan](capabilities_implementation_plan.md) holds every open task; [`followup.md`](followup.md) holds every open decision. This document holds no task lists.
- **Current code** describes the inspected implementation, not a desired contract. Recheck reachability and source details before implementing a slice.
- **CONFIRMED** identifies maintainer-selected semantics, also reflected in the canonical contract. **INTERIM** identifies a temporary direction whose detailed enforcement still needs decisions.
- **Recommendation** identifies a proposed direction, not an accepted decision. Alternatives are retained for context, not as competing implementation instructions.
- **OPEN / DECISION REQUIRED** identifies an unanswered question that must be settled before dependent implementation.

When a decision is approved, record its chosen semantics, rationale, and compatibility impact in the canonical contract first, then reconcile this document and the implementation plan before changing dependent code.

## Working hypothesis: thin handles, authoritative kernel checks

The motivating position is that userspace representations should not closely track kernel object lifecycle: after an object is disposed, invoking its userspace capability should return an error.

**CONFIRMED** ([contract](nucleus_capabilities.md#vocabulary-and-identity)): handles name the capability incarnation obtained by the caller and need not monitor object liveness. The remaining qualification concerns mapped-memory APIs and precise completion contracts:

> Userspace handles need not mirror liveness, provided the kernel validates the intended identity and authority on each invocation. Direct memory access and ownership-consuming operations need additional contracts.

Not tracking whether an object is alive is different from not knowing which incarnation of an object or slot a handle names. A userspace liveness query is at most an observation; it cannot replace validation at the operation's authorization/commit boundary.

The desired separation is:

- A **handle** names the particular capability incarnation obtained by the caller, never an unrelated future occupant of its slot.
- A **capability entry** carries authority over a resource or delegation scope.
- A **resource** has independently managed identity, state, and physical lifetime.
- An **in-flight operation or mapping** can retain dependencies after its initiating handle disappears.

Phantom types improve ergonomics, not authority or lifetime enforcement. Copying a Rust handle is not kernel capability derivation.

## 1. Stale handles and identity reuse

### Slot-only counterexample and current status

The original [`Key<T>`](../libs/object/src/key.rs) contained only `KeySlot` and `PhantomData<T>`. Its slot-only transport allowed this counterexample:

1. Slot 12 contains Invocation A.
2. Userspace retains a key naming slot 12.
3. A's entry is deleted and the slot is reused for Invocation B.
4. The retained key invokes slot 12 and can operate on B.

Even a type check cannot distinguish two Invocations. This need not escalate authority across address spaces—B is already in the caller's table—but it can use replacement authority unintentionally. Thus stale invocation did not necessarily fail under the former slot-only representation.

Implemented: keys carry a 32-bit incarnation alongside the table-relative slot address through every wrapper, the transport and dispatch. Lookup and removal check the expected incarnation, deletion keeps the counter, and installation advances it only at commit; a slot whose counter reaches `u32::MAX` is exhausted rather than wrapped. Bootstrap hands recipients the actual issued keys. This closes slot reuse within a table's lifetime; object and Thread retirement identity are separate (§3, §6).

### Alternatives and selected semantics

The incarnation guarantee is now selected; a slot-only current-occupant API is not the chosen contract. **Maintainer clarification:** ordinary keys are local to the current Thread's implicit KeyTable — the table associated with the Thread's current AddressSpace — and carry a guarded table-relative slot plus incarnation. Numeric key transfer alone conveys no authority; authorized derivation/installation produces a recipient-local key. **Follow-up:** initially use a 32-bit slot and 32-bit incarnation, without freezing the total key size permanently. If type is embedded later, its 8 bits come from the slot field (24 slot bits, 32 incarnation bits), not incarnation. Capacity is fixed for now through an easily changed constant; runtime resizing is outside initial scope. **Approved packing:** slot in low 32 bits and incarnation in high 32 bits, with no initial type tag. Typed handles are non-owning. Live Threads may not rebind to a fresh logical invocation table with reset counters. Kernel allocation references use pool identity/index and 64-bit generation, with no generation wrap or metadata-identity reset; concrete ownership and Thread integration remain work. Independent shared-object lifetime validation remains required. Generation wrap must not silently resurrect stale identities; Move invalidates the source on commit and yields a destination-local key. The included ABI/bootstrap paths have migrated together without a slot-only fallback; excluded sketches remain unmigrated, unsupported design material.

| Choice | Consequences | Complexity |
|---|---|---|
| Slots name their current occupant, like file descriptors | Accept reuse hazards; userspace coordinates slot ownership | Low kernel complexity |
| Exclusive userspace slot allocator with owned slots and borrowed handles | Prevent accidental reuse while handles exist, provided every mutation follows that allocator | Medium; requires enforceable runtime discipline |
| Kernel-checked slot incarnation supplied with invocation | Retained handles fail after replacement without monitoring liveness | Medium cross-layer change, including ABI/bootstrap |

**CONFIRMED** ([contract](nucleus_capabilities.md#approved-key-identity-and-wire-package)): invoking an altered or invalidated incarnation returns an explicit inconsistency error, never a silent refresh against replacement authority.

Authorized resource-state changes are not automatically capability replacement. In particular, origin-authorized Frame remapping is a valid operation even when mapping location changes. **Selected initial scope (3A=A):** no in-place rights/badge mutation. Derive attenuated authority into another slot and optionally delete the original; CopyDerive preserves badges and rebadging is deferred. Future in-place mutation semantics are not a prerequisite for this initial slice.

**Selected exhaustion behavior (3B=A):** a slot unable to issue a fresh 32-bit incarnation is permanently unavailable for further installation within that table lifetime; existing-capability deletion remains possible and other slots remain usable. No silent wrap or whole-table retirement is implied. The approved key package starts unused counters at 0, issues 1 on first installation, advances only at successful installation, retains counters on deletion, and reports exhaustion with status 28. Replacing/rebinding the table must not reset stale-key protection.

**INTERIM** ([contract](nucleus_capabilities.md#mapping-and-sharing)): Frame deprovisioning fully revokes the capability and does not reuse that slot for Frames.

Three distinct identity problems must be covered:

- **Slot incarnation:** whether a local handle refers to a replaced entry.
- **Object/thread incarnation:** whether a kernel reference refers to recycled storage.
- **Selective delegation-scope validity:** relevant only if one branch of capabilities to a still-live object must be revoked while other branches survive.

Derived capabilities reference the same kernel object. An authoritative object-generation mismatch is sufficient to reject all old invocations of that retired object, without walking a capability ancestry chain. The branch distinction matters for selective revocation, not for detecting global object retirement. A generation is not a secret or a substitute for authorization, and object generation alone does not detect replacement of a slot with a different valid capability.

Open work for this section is tracked in [Phase 4](capabilities_implementation_plan.md#phase-4--capability-storage-and-thread-lifetime); open decisions are in [`followup.md`](followup.md).

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

Recorded in the contract's [lifecycle and authority contract](nucleus_capabilities.md#lifecycle-and-authority-contract): Retype-origin capabilities carry delegable lifetime-control permission, the creator keeps control after donation, kernel retirement is separate from manager subtree cleanup, and abandoned allocations may leak without kernel recovery. Two consequences: capability reference counting alone would not account for pending invocations, mappings or internal relationships; and a pin that keeps storage safe is **not** permission to keep using revoked authority — physical lifetime and logical authorization stay separate.

Open work for this section is tracked in [Phase 4](capabilities_implementation_plan.md#phase-4--capability-storage-and-thread-lifetime) and [Phase 5](capabilities_implementation_plan.md#phase-5--memory-and-safe-reclamation); open decisions are in [`followup.md`](followup.md).

## 3. Guarded kernel access and storage lifetime

### Current code

- [`KeyEntry`](../kernel/nucleus/src/api/key_entry.rs) stores either a checked pooled-object identity (pool tag, index, `u32` generation) that is validated against the pool's authoritative metadata before any access, or an inline carved-region description (KeyTable, Frame, Untyped). Carved objects have no retirement identity. The contract specifies 64-bit generations.
- [`ObjectPool`](../kernel/nucleus/src/objects/object_pool.rs) is one carve from an Untyped's unused watermark range: per-slot metadata (`SlotMeta`: Free/Live/Retired plus a retained generation that never wraps) followed by the object storage, sized by the capacity given at creation. Pools are created only at bootstrap. A stale or retired identity is rejected as `InvalidOperation`, not yet as the `ObjectRetired` inconsistency.
- [`KeyTable`](../kernel/nucleus/src/objects/key_table.rs) rejects null, occupied, out-of-bounds and exhausted insertion without changing occupancy or losing the submitted entry; lookup and removal require an incarnation-bearing selector. KeyTables are kernel-private carved objects created by `Untyped.Retype`.
- The nucleus entry path uses [`IRQSafeNullLock`](../libs/locking/src/lib.rs), which masks interrupts rather than providing multicore exclusion; execution is single-core.

Returning an error requires safe validation **before** dereferencing an object. Comparing against metadata in already-freed backing is not a valid generation check.

### CONFIRMED: correctness before representation optimization

Recorded in the contract's [key tenets](nucleus_capabilities.md#key-tenets) (correctness before representation optimization); the skill requires the matching layout audit with every representation change.

Open work for this section is tracked in [Phase 4](capabilities_implementation_plan.md#phase-4--capability-storage-and-thread-lifetime); open decisions are in [`followup.md`](followup.md).

### Selected guarded-access direction and remaining representation work

**CONFIRMED** ([contract](nucleus_capabilities.md#selected-local-key-and-storage-foundation)): single-core kernel execution with SMP deferred, short-lived guarded references from an owning access context, and all managed storage from accounted Untypeds.

Typed pools remain a useful starting point, with representation free to change under the correctness-first rule:

1. Persistent capabilities naming pooled objects carry checked object identities, conceptually `(pool, index, generation)`. The explicit `CurrentReturnOnly` Thread selector names no concrete object and rejects object-identity extraction.
2. Authoritative allocation metadata records allocation, generation, and retirement state, with its own valid backing lifetime.
3. An owning access context resolves identities and provides short-lived guarded access.
4. Multi-object operations resolve same-table/same-object aliases explicitly.
5. Object references and incompatible guards end before scheduling or context switching.
6. Pending operations retain checked identities and explicit reservations, not borrowed Rust references.

**SELECTED — D3 concrete guarded access:** capabilities naming pooled objects store only a checked object identity (pool tag, pool index, allocation generation); entries hold no raw object pointer. Named Thread capabilities use `ThreadSelector::Named(ObjectId)`; the separate `ThreadSelector::CurrentReturnOnly` selector has no concrete Thread identity to resolve and `object_id` extraction returns `InvalidOperation`. It still requires ordinary checked caller-table/key lookup, not a fabricated zero identity or lookup bypass. The owning access context computes object addresses from pool bases after validating per-pool authoritative slot metadata (allocation state + generation + retirement), whose backing is stable and independent of object storage — validation always precedes dereference. The context is constructed once per invocation under the kernel lock, is `!Send`, and hands out guards that borrow it, so the borrow checker enforces guards ending before scheduling. Multi-operand operations use explicit pair-resolution forms that reject aliased mutable operands up front; a lock serializes executions but does not by itself prevent two operands within one execution from naming the same object. Mutable resolution exclusively borrows the pool for the invocation, so the borrow checker prevents holding two mutable guards into one pool; clients requiring two objects from the same pool must request them through the pair-resolution API (`resolve_pair_mut`) rather than sequential mutable resolution. One pool per type tag is the invariant making this sufficient; introducing multiple pools per type requires revisiting cross-pool alias enforcement (for example a runtime alias set). Single-core remains selected; SMP is not an open choice. **Remaining D3 work:** Untyped-backed pool ownership, kernel-private KeyTable backing, and safe pool-backing lifetime (Phase 5, with D1/D6 for protection bindings); thread lifecycle integration; per-kind access details. The boot Untyped allocation and the shared `RegionPayload::reserve` watermark-allocation primitive are established (Kickstart's `carve_region` and `ObjectPool::carve` carve the initial `Nucleus` and all six pools from the boot Untyped's unused watermark range; runtime Retype reserves through the same primitive); syscall entry resolves the caller's table through the `Access` context. The boot table is carved kernel-privately by Kickstart, runtime `Untyped.Retype` creates further tables from an Untyped's unused watermark range, and carved-table resolution uses address-guarded `resolve_carved` forms with alias rejection. Pool retirement/reuse semantics remain.

**Capability construction implementation:** `KeyEntry::from_id` returns `Result<KeyEntry, CapError>` and accepts only known identity-backed kinds. NULL, UNTYPED, FRAME, KEY_TABLE and INVOCATION are rejected with `InvalidObjectType` before initializing a payload; dedicated constructors supply those kinds' payloads. Thread construction initializes `ThreadSelector::Named(id)`, while `new_thread_return` initializes `CurrentReturnOnly` with empty rights. `thread_selector` and `is_thread_return_key` inspect the form without object resolution, and `object_id` on the sentinel returns `InvalidOperation`. Thread.Retire rejects the sentinel before checking `RETIRE` rights or extracting identity. The selector is 12 bytes/alignment 4; `KeyEntry` is 64 bytes/alignment 32 with a 40-byte payload union. The dedicated 40 B/alignment-8 Invocation payload stores target AddressSpace identity, mandatory `NonZero<u64>` entry and immutable validated `InvocationStackExtent` (24 B/alignment 8). `KeyEntry::new_invocation` requires that validated extent; `invocation_stack_extent` exposes a kind-checked copy. The 32 B KeyTable header is unchanged; entries use a 64 B stride with separate 4 B counters. `KeyTable::carve_size(size_bits)` derives the full aligned carve from the actual types; 256 entries require 17,440 B. Runtime Retype, bootstrap, archive and fixture backing/counter offsets follow that calculation. These are code/layout facts, not new test-run results or PPC dispatch.

**AddressSpace association implementation:** `KeyTableBinding` is an immutable typed handle issued only by `unsafe KeyTable::binding` from a fully initialized private carve. Its safety contract retains the whole backing at the same address, without reclamation/reinitialization for every surviving copy; this is the interim accepted-leak guarantee, not a generation-bearing reclaimable-table identity. AddressSpace provisioning requires the binding, dispatch validates AS incarnation before dereference, and SELF address/capacity is checked against binding/header while SELF supplies the guard. Threads share the AS table and creating a second Thread is table-neutral. QEMU dispatch, boot, backing-bounds, and full lint validation are recorded in the plan's AddressSpace-associated KeyTable prerequisite section. General hostile-EL0 backing enforcement and future table reclamation remain unchecked below.

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
| Activate an address space | Mapping-context authority (`MAP`), a bound root and ASID; the execution-budget requirement lands with Phase 7 Thread control |

**Interim Activate selection:** the active `AddressSpace.Activate` is the translation-context installation step only — `MAP` on the invoked AddressSpace capability (mapping-context authority, consistent with root installation and ASID binding), a bound root and ASID, and the current caller's own AddressSpace only. The execution-budget requirement lands with full Thread Start/Suspend/Resume (Phase 7, D8).

**Recommendations:**

- Full table management is stronger than accepting a capability into a reserved receive slot. Keep destination authority explicit; narrower installation windows are a possible later refinement.
- Badges are authority metadata, not arbitrary caller identity. Define who may mint/rebadge a sender view; ordinary delegation must not silently allow impersonating another authority-bearing view.
- Copy is per-kind: Untyped cannot duplicate independent allocation watermarks; Invocation derivation must not amplify its target or rights; Time must conserve budget. **Confirmed Frame rule:** checked Copy duplicates capability authority, not a live mapping association; it is not Map. B maps the same physical frame through a separate Map operation, potentially at a different virtual page/AddressSpace. A move preserves the binding needed for teardown.
- Validate and reserve a destination before removing source authority. Resolve aliases before constructing references or mutating operands.

**CONFIRMED** ([contract](nucleus_capabilities.md#authorization)): table management follows ordinary source/destination KeyTable permissions with no manager-identity exceptions; Kickstart establishes the first Untypeds and tables, then hands authority on.

Open work for this section is tracked in [Phase 4](capabilities_implementation_plan.md#phase-4--capability-storage-and-thread-lifetime); open decisions are in [`followup.md`](followup.md).

### Who tracks revocation?

**CONFIRMED** ([contract](nucleus_capabilities.md#lifecycle-and-authority-contract)): components granted management rights derive, install and manage capabilities directly and own the bookkeeping, joining that composition's TCB; multiple or hierarchical managers are policy.

A Key selects a slot/incarnation; KeyTable capabilities provide authority for table operations. The existing Vesper abstraction already expresses the distinction between using an object and managing entries that name it. A component without direct management authority can call a manager service instead. A component explicitly granted direct derivation power is not required by the nucleus to register afterward with a central KeyMaster; its bookkeeping obligations belong to the entrusted composition.

The kernel supplies checked source/destination authorization, per-kind transitions, rights attenuation, lifetime validity, and quiescence/reuse mechanisms. Composition-scoped TCB membership does not grant unconfined execution or bypass kernel memory protection. This is the selected **SPeCK-like resource-management model**, not wholesale adoption of Composite's layouts or every implementation detail. The table below lists alternatives; kernel-owned tree management and a mandatory kernel ancestry chain are not the chosen direction.

| Model | Benefits | Costs |
|---|---|---|
| Composition chooses one entrusted manager | Smaller kernel derivation machinery; clients use its services | Direct management authority must be granted consistently with that policy; leaks/recovery remain OS concerns |
| Kernel tracks derivation scopes | Supports direct delegation between mutually distrustful components | Charged metadata, traversal, bounded cancellation and reclamation |
| Coarse allocation/delegation scopes | Simpler initial revocation mechanism | Broader disruption; less selective reclamation |

**Selected split:** KeyMaster tracks the tree; derived caps across tables reference a common object identity. Kernel object retirement changes that object's validity/generation, rejecting all previous cap invocations. KeyMaster then cleans up dead entries and derivation metadata in the background. Shallow trees are a performance expectation, not a requirement to traverse ancestry on every invocation.

An object-wide epoch does not provide selective branch-only invalidation while sibling caps to the same object remain valid. If that separate operation is required, KeyMaster must use an explicit invalidation protocol; the exact primitive/completion boundary remains open. Do not add kernel ancestry metadata as a presumed prerequisite for the already-selected object-retirement mechanism.

Lazy object invalidation does not remove PTEs or cancel all retained work by itself. **The authorized library OS/resource manager owns unmap-before-invalidation orchestration.** Trust follows explicitly granted capabilities, not a blanket assumption that all Threads or libOS instances cooperate. It must not invalidate the only usable cleanup authority and then expect a malicious recipient to cooperate. Premature-reuse checks and the kernel/manager completion protocol remain D2/D6, but the D1 threat model requires confinement even when a recipient bypasses its libOS. Accepted allocation leaks must not become conflicting physical reuse.

Granting direct management authority also entrusts the recipient with bookkeeping. Multiple or hierarchical managers distribute this responsibility through composition policy. No universal central registration protocol or name-based manager privilege is implied. Exact manager-side synchronization, partial-failure handling, and coordination remain work for the chosen composition, not a new kernel policy mandate.

## 5. Direct userspace memory access is not a fallible invocation

### Current code and limitation

Included [`DcbView`](../libs/object/src/domain.rs) constructs references into shared mapped memory. The [`Buffer` wrapper](../userspace/buffer/buffer.rs), intended functionality currently excluded from compilation, creates ordinary slices; it lives under `userspace/buffer/` because Buffer is a userspace/libOS construct.

“Direct references” means `&[u8]`, `&mut [u8]`, or `&DomainControlBlock` produced by these userspace wrappers after mapping—not raw pointers granting authority across the kernel boundary. The control interface remains capability-based; subsequent loads/stores are not capability invocations and cannot return the inconsistency status.

`MappedSlice` and `MappedSliceMut` already invoke Unmap on Drop. **Intended ownership design:** create and own a private capability/mapping inside the guard, allow no further derivations or independently usable management aliases, and release the mapping on Drop. The current sketch does not yet implement this: `map_slice[_mut](&self)` borrows an existing BufferKey and stores that borrow rather than creating/owning a dedicated capability.

**CONFIRMED** ([contract](nucleus_capabilities.md#allocation-and-representation)): Drop cleanup uses an incarnation-bearing key, so a stale key is rejected rather than unmapping a replacement. The current sketch's destructor still discards errors.

There are two different exclusivity questions: ownership of the capability/mapping, and access to the physical bytes. No derivations from the guard-owned capability addresses the first. It does not prove that the physical frame has no aliases in other AddressSpaces, ancestor retirement authority, or device writers. D1 now prohibits multiple virtual aliases within one AddressSpace, but permits cross-AddressSpace writable sharing.

**CONFIRMED** ([contract](nucleus_capabilities.md#protection-requirements-and-boundaries)): authorized revocation is not vetoed by a client's Rust borrow; access after withdrawal to a still-unmapped address faults.

**Preferred Rust direction:** explicit unsafe caller obligations for reference-producing access, because the kernel cannot generally promise cross-AddressSpace exclusivity of shared resources. This is not a decision to make every mapping operation unsafe. The exact unsafe API and its full-borrow obligations remain to be designed; raw/fbuf protocol access may be preferable where an ordinary reference cannot be justified. Safe wrappers may exist only where they actually enforce stronger conditions. The kernel must remain memory-safe and isolate unrelated authority even if the application violates its unsafe contract.

**Technical correction — read-only is not immutable backing:** read-only PTE permissions prevent stores through that particular mapping. For example, A can map frame F read-only while B maps F read-write; B's permitted write changes the bytes A observes. DMA and kernel writers can also change them. This is consistent with hardware read-only protection, but inconsistent with calling the backing immutable. A read-only DCB mapping updated by the kernel is an existing example. Ordinary `&[u8]` needs no conflicting mutation for its borrow, not merely an inability to write through A's PTE. An immutable-sharing mode must establish a stronger backing/protocol guarantee.

**Stale raw pointers after VA reuse:** the current rule is in the contract's [protection requirements and boundaries](nucleus_capabilities.md#protection-requirements-and-boundaries); stronger guarantees are [future work](followup.md#future-work).

### What an ordinary `&[u8]` requires

The standard library's [`slice::from_raw_parts` safety contract](https://doc.rust-lang.org/std/slice/fn.from_raw_parts.html#safety) is the relevant boundary when a mapping wrapper creates a slice. For `&'a [u8]` it requires:

1. A non-null pointer, correctly aligned, valid to read all `len` bytes. Non-null/alignment still apply to empty slices; `u8` alignment is one byte.
2. Consecutive initialized bytes in a single valid allocation, with correct bounds/provenance. Every initialized bit pattern is valid for `u8`, but uninitialized memory is not. Adjacent mappings/allocations are not automatically one Rust allocation. Physical contiguity is not required; the mapping abstraction must establish the virtual allocation it exposes.
3. Total byte length at most `isize::MAX`, without address arithmetic wrapping.
4. Memory that remains valid for the entire borrow: no deallocation, unmapping, or replacement that invalidates the reference while it is live. The wrapper must tie the slice lifetime to the actual access contract, not manufacture permanence.
5. No mutation of those bytes for the borrow's duration. The documented interior-mutability exception concerns `UnsafeCell`; ordinary `u8` elements provide no such exception. A writer in another AddressSpace, DMA, or the kernel can violate this even if the observer's PTE is read-only. Multiple shared readers are allowed, but competing exclusive/mutating access is not.

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

**OPEN / DECISION REQUIRED — D1/D3/D5/D6/D7:** implement private guard-owned mappings and specify the preferred unsafe/fbuf protocols under authoritative revocation. All three sharing modes are required: exclusive ownership, immutable sharing, and synchronized mutable sharing. Scheduler-shared record pages and ordinary revocable buffers have different lifecycles; shared-page availability does not imply immutable contents.

Open work for this section is tracked in [Phase 5](capabilities_implementation_plan.md#phase-5--memory-and-safe-reclamation); open decisions are in [`followup.md`](followup.md).

## 6. Thread identity and DCB observation

### Current code

The kernel's [`DcbPages`](../kernel/nucleus/src/objects/domain.rs) owns the DCB pages and assigns global `DomainId`s; lookup checks page availability but not an incarnation. The shared ABI's [`DcbView`](../libs/object/src/domain.rs) maps a kernel-owned view read-only to userspace. Both are scheduled for removal. Capability entry rejects an absent current Thread and checks the exception origin; `Nucleus::current_thread` is still a bare pool index.

The selected direction — scheduler-owned record pages shared through `Scheduler.ShareRegion`, the root scheduler established by Kickstart, root-only Thread creation and donation — is in the contract's [Thread and AddressSpace contracts](nucleus_capabilities.md#thread-and-addressspace-contracts). Composite's upcall-based scheduling and scheduler-owned thread structures (Parmer, TECS 2013) are design evidence, not a wire-contract source.

Open work for this section is tracked in [Phase 4](capabilities_implementation_plan.md#phase-4--capability-storage-and-thread-lifetime) (scheduler-shared records) and [Phase 7](capabilities_implementation_plan.md#phase-7--time-and-userspace-scheduling); open decisions are in [`followup.md`](followup.md).

## 7. Pending operations, PPC Invocation, and Time

### Current state

Notification/EventCount waits, PPC Call/Return and fault delivery are implemented as specified in the contract's [communication and deferred completion](nucleus_capabilities.md#communication-and-deferred-completion) section; Time remains an excluded sketch.

For a blocked caller, “the next invocation fails” is inadequate: there may never be another invocation. Retirement must arrange an explicit continuation outcome. Revocation/timeout cannot undo already committed effects or imply that a delivered request was never processed.

### CONFIRMED vocabulary for aborted work

Recorded in the contract's [aborted-work vocabulary](nucleus_capabilities.md#aborted-work-vocabulary): rejected before admission, cancelled before commit, completed, outcome unknown; the nucleus provides local mechanisms only.

Open work for this section is tracked in [Phase 6](capabilities_implementation_plan.md#phase-6--deferred-completion-and-ipc) and [Phase 7](capabilities_implementation_plan.md#phase-7--time-and-userspace-scheduling); open decisions are in [`followup.md`](followup.md).

### PPC execution-context research (2026-09-28)

Evidence for the selected PPC execution-context directions (canonical contract, communication section). Sources: Fluke 0.5 local sources (`kernel/csw.h`, `kernel/x86/nonblocking/{csw.c,trap.S}`, `kernel/ipc.c`, `kernel/ipc.h`, `kernel/pclsr_subr.c`, `kernel/preempt.h`, `kernel/wait.c`); Composite `gwsystems/composite` `main` (`src/kernel/capinv.c`, `src/kernel/include/inv.h`, `src/kernel/include/thd.h`, `src/components/lib/component/arch/x86_64/cos_asm_simple_stacks.h`, `src/components/lib/stubs/arch/x86_64/cos_asm_stubs.h`, `src/platform/i386/boot_comp.c`); prior-art survey (primary sources where reachable, memory otherwise, marked).

#### Fluke 0.5: interrupt-kernel trap path; IPC is a handoff

- `CONFIG_NONBLOCKING` selects an interrupt kernel: one kernel stack per CPU, set once at boot; `csw_switch` (`kernel/x86/nonblocking/csw.c`) performs no stack switch — page-directory switch plus a per-CPU current-thread pointer at a fixed offset from the per-CPU stack base.
- The trap path (`kernel/x86/nonblocking/trap.S`) copies all GPRs into `TH_EXC_STATE` embedded in the thread struct; `return_to_user` reads them back from the (possibly different) current thread and `iret`s. The kernel stack holds nothing per-thread. Kernel-mode traps can only restart: kernel code cannot block or migrate. Interrupts nest on the single stack; unsafe work is deferred to preempters (`kernel/preempt.h`) running at return-to-user safe points.
- Fluke IPC is a handoff, not same-thread migration: `ipc_call` ends in `thread_handoff(client, WAIT_IPC_CALL, server)` (`kernel/ipc.c`, `kernel/wait.c`); a separate server thread wakes with in-kernel registered landing state (`ipc_state`: `server_esp`, `server_*_eip` in `kernel/ipc.h`). No nested migration exists. Continuations are PCLSR-style (`kernel/pclsr_subr.c`): the kernel writes user-level entry-stub EIPs and processes `KR_*` restart codes; syscall EIPs are masked for restartability.
- Adopted for PPC: per-thread exception state in the TCB, restart discipline, preempter-style deferred work. Fluke is not precedent for same-thread migration or nested PPC frames.

#### Composite: invocation stack and sret mechanics (source-verified)

For Vesper's stack policy, the source-verified Composite implementation's key limitation is that the kernel records/restores client SP but does not validate the target SP: the target stub obtains a stack from a per-component pool. Vesper's selected refinement is to publish an extent with each Invocation at creation and check the Call-time target SP against it, without claiming that this proves the extent is mapped writable.

- Composite's capability tables (`captbl`s) are component-level: a component has its own table, and its threads execute with that component's capability namespace; they are not independently assigned one captbl per thread. This is precedent for context-dependent table selection, not one table per hardware address space: co-resident components can retain distinct tables, as detailed below.
- `sinv_call` (`src/kernel/include/inv.h`) pushes the client's IP/SP onto a bounded per-thread invocation stack in the TCB (`THD_INVSTK_MAXSZ 16`; `struct invstk_entry { comp_info, sp, ip, ulk_stkoff, protdom }`), switches page tables, redirects the trap-frame IP to the target entry, and passes a token (delivered in the SP register; the server stub immediately switches to a freelist stack). The kernel never switches SP: the client stub places its return address in the `invret` register; the callee's stack comes from a userspace per-component stack pool (`custom_acquire_stack`, `cos_asm_simple_stacks.h`), and the kernel only records and restores (ip, sp) pairs. Overflow fails at call time with an error to the caller.
- `invstk_top` is cached in per-CPU `cos_cpu_local_info` and committed to the TCB on every thread switch (`thd_current_update`); full `pt_regs` are copied to the TCB on preemption. A preempted thread's later sret finds the right entry purely via the restored top — no matching or scanning.
- sret invocation: the syscall register packs cap id and op (`(cap+1) << 16 | op`); cap id 0 is the hardwired `RET_CAP` sentinel. The fast path in `composite_syscall_handler` (`capinv.c`) executes `sret_ret` for cap 0 before any captbl lookup, so every component can return regardless of its capability table. The slow path handles an actual `CAP_SRET`-typed capability, but the cap pointer is not passed to `sret_ret` — no cap-versus-invstk check exists.
- `cap_sret` is payload-less (`struct cap_sret { struct cap_header h; }`), minted exactly once at boot into the boot component's captbl (`sret_activate`, `src/platform/i386/boot_comp.c`); userspace cannot mint more (`CAPTBL_OP_SRETACTIVATE` returns `-EINVAL`). It is copyable but decorative.
- `sret_ret` pops the invocation stack. Underflow yields a defined crash (`0xDEADDEAD` return value with ip=0/sp=0); a failed liveness check on the return-target component yields `-EFAULT` with the callee still executing; success switches page tables and protection domain, restores the saved (ip, sp), and moves the `invret` register into the return register. Validation is exactly: invstk non-empty, target alive. No per-invocation token, strict LIFO, and a double sret silently skips a frame.

#### Composite keytable access across shared/merged protection domains (source inspection)

**Correction to the analogy:** a Composite component and a hardware address space are separate identities. At source pin `3ef8f8c4d3296624640e6f3bd00801054d8350a3`, [`comp_activate`](https://github.com/gwsystems/composite/blob/3ef8f8c4d3296624640e6f3bd00801054d8350a3/src/kernel/include/component.h) binds `comp_info.captbl`, `pgtblinfo.pgtbl`, and `pgtblinfo.protdom` independently from the supplied table capabilities and protection-domain value. Shared-VAS provisioning (`cos_comp_alloc_shared` in `src/components/lib/kernel/cos_kernel_api.c`, `crt_comp_create_in_vas` in `src/components/lib/crt/crt.c`) permits components to retain separate capability tables while using a shared page-table root. Sharing mappings does not union the tables.

Current typed-cap kernel:

- [`composite_syscall_handler`](https://github.com/gwsystems/composite/blob/3ef8f8c4d3296624640e6f3bd00801054d8350a3/src/kernel/capinv.c) obtains `ci = thd_invstk_current(...)`, then performs `captbl_lkup(ci->captbl, cap)`; the slow path uses the same component-selected table. An ordinary syscall supplies a capability number, not an arbitrary table pointer. Explicit management of another table requires table authority. The cap-0 return sentinel is a special bypass, not ordinary table lookup.
- For kernel-mediated calls, `sinv_activate` copies the destination's `comp_info` into the installed invocation capability; [`sinv_call`](https://github.com/gwsystems/composite/blob/3ef8f8c4d3296624640e6f3bd00801054d8350a3/src/kernel/include/inv.h) pushes that destination context onto the same Thread's kernel-owned invocation stack. Subsequent lookups select its table. `sret_ret` pops back to the source context. The Thread does not keep one immutable authority table for its entire lifetime.
- For user-level calls among co-resident components, the x86-64 callgate pushes the relative invocation capability and SP onto a user-level invocation stack and switches PKRU before jumping; no syscall or CR3 switch occurs on this path. At the next kernel entry, [`thd_invstk_current`](https://github.com/gwsystems/composite/blob/3ef8f8c4d3296624640e6f3bd00801054d8350a3/src/kernel/include/thd.h) starts with the current kernel frame and reconciles user entries after its saved `ulk_stkoff`. [`ulinvstk_current`](https://github.com/gwsystems/composite/blob/3ef8f8c4d3296624640e6f3bd00801054d8350a3/src/kernel/ulinv.c) walks them with `captbl_lkup(ci->captbl, ent->sinv_cap)`, requires `CAP_SINV`, and follows each capability's destination `comp_info`. The final logical component supplies the syscall table; this is not a union or a selection by thread ID.
- Example: A and B share a page-table root but have tables KA and KB. A invokes B through a user callgate, then B makes a kernel syscall: replay follows A's installed A→B capability and selects KB. If B kernel-invokes C, the kernel pushes C's context; Return restores the source kernel frame/UL offset and protection state, and the next ordinary lookup reconstructs B again. `thd_invstk_curr_comp` explicitly keeps the hardware page table from the kernel frame while separately returning the reconstructed logical component. Thus table selection can change without changing the page-table root. Kernel-mediated `sinv`/`sret` nevertheless call `pgtbl_update`; the inspected x86 implementation writes CR3 even for an unchanged root.
- **Authentication limit:** UL replay validates a capability-reachable chain from the kernel frame, not an independent correspondence between that chain and the currently executing instruction address or PKRU. The stack is user-writable with a protection-key assignment. The inspected callgate has unfinished authentication failure branches and setup uses debug tokens. Logical table selection alone is not evidence of hostile-native confinement between co-resident components. This is static inspection, not runtime validation or a complete callgate security audit.

Legacy MPD merging is a different mechanism (pin `043980416d660da1e0910549aa3600e6b59ed055`):

- [`spd.h`](https://github.com/gwsystems/composite/blob/043980416d660da1e0910549aa3600e6b59ed055/src/kernel/include/spd.h) keeps `caps`/`user_cap_tbl` on each base `spd`, while `composite_spd` describes merged membership and page tables. `spd_add_caps` in `src/kernel/spd.c` changes isolation levels and UCap invocation pointers to direct calls; it does not concatenate capability tables. Its intra-composite rule is explicitly **symmetric trust**, not hostile isolation between members.
- Kernel invocation frames retain both the entry base `spd` and `current_composite_spd`; direct calls inside the merged domain do not update that kernel frame. In this pinned [`ipc_walk_static_cap`](https://github.com/gwsystems/composite/blob/043980416d660da1e0910549aa3600e6b59ed055/src/kernel/inv.c), lookup uses `thd_curr_spd_thd(thd)->caps[capability]`, and the helper returns the frame's entry `spd`. A direct A→B call inside a merged domain therefore does not itself change this kernel lookup to B's table. `thd_get_thd_spd` comments acknowledge entry-SPD inaccuracy after merging and discuss IP lookup, but its executable body still returns the entry SPD. Do not present this snapshot as a verified solution for arbitrary B-originated egress after direct intra-domain calls.
- [`thd_validate_get_current_spd`](https://github.com/gwsystems/composite/blob/043980416d660da1e0910549aa3600e6b59ed055/src/kernel/include/thread.h) checks a supplied SPD's membership in the current composite, not exact executing-component identity. Separate base tables are consequently not proof of mandatory intra-merged-domain authority separation. The [MPD paper](https://www2.seas.gwu.edu/~gparmer/publications/mpd_rtss07.pdf) distinguishes discretionary user-capability control from mandatory protection when isolation is required.

**Vesper consequence and accepted direction:** one KeyTable per AddressSpace remains the selected contract. Bind that table during AddressSpace construction/provisioning (option A), independently of Thread creation; no runtime kernel allocation or separate bind operation is selected. Composite supports provisioning-time association and same-thread context-dependent table lookup, but not an equation of component identity with page-table identity. Co-locating procedures in one Vesper AddressSpace gives them its shared table; distinct logical-component tables within one hardware address space would require a separately selected, trustworthy active-authority context. That additional mechanism is not adopted here. The immutable issued carved-table association and accepted-leak backing-lifetime obligation are implemented as described in §3 and the §7 audit. Future reclamation-safe table identity and full hostile-EL0 backing enforcement remain implementation work.

#### SPeCK paper versus implementation

The SPeCK paper/dissertation could not be fetched in this pass; paper claims below are recollection, implementation claims above are source-verified. Recollection: SPeCK describes invocation as automatically creating a per-invocation return capability that authorizes the return. The implementation differs materially: per-invocation state lives in the TCB invstk entries, and the "return capability" is a static payload-less type plus the cap-0 sentinel that bypasses capability lookup entirely. The pre-SPeCK design (Composite dev manual: per-component ucap tables, `COS_THD_INV_FRAME` introspection) was replaced by typed capabilities plus the invstk. The ULK user-level invocation stack (user-writable `ulk_invstk`, MPK domain switches, kernel re-validation of every hop) postdates SPeCK. The real security property is per-thread kernel state: a thread can return only to its own invocation chain, never to a context it was not called into, and never from a non-top depth; a third party cannot complete another's call because any control transfer to it pushes a new frame.

#### Thread-migration prior art

Verified sources: Coyotos archive (`vsrinivas/coyotos` `src/web/docs/ukernel/irq-and-smp.xml`, `eros-comparison.xml`), the archived KeyKOS Architecture documentation (agorics.com via the Internet Archive: `architecture/{Domain.States,Intro.Domains,Messages,Three.Special}.html`), Gamsa's Tornado thesis (Ch. 5) and the Tornado OSDI'99 paper (Wayback of eecg.toronto.edu), the K42 source mirror (`github.com/jimix/k42`, branch `kitchsrc`: `lib/libc/sys/ppccore.H`, `os/kernel/init/ppcbak.H`, `DispatcherDefaultAsm.S`), the K42 EuroSys'06 and IBM SJ 44(2) papers, the Opal TOCS'94 paper (`homes.cs.washington.edu/~levy/opal.pdf`), Roscoe's and Barham's Cambridge theses (UCAM-CL-TR-376/-403) and the Nemesis JSAC'96 paper, the NOVA source (`github.com/udosteinberg/NOVA`, current release branch and the 2010-05-12 paper-era commit) plus the archived NUL userspace documentation, the Mungi PDX paper (IWOOS'96), multicians.org (gates, ring brackets, stacks), VMS privileged-code documentation, illumos doors (`door_call(3C)`, `sys/door.h`), seL4 manual (`seL4/seL4` `manual/parts/{ipc,threads}.tex`), Pistachio whitepaper. Others from memory, marked [M].

| System | Same-thread migration? | Continuation | Target stack | Return | Kernel stack |
|---|---|---|---|---|---|
| LRPC (Bershad 1990) [M] | yes | cross-domain A-stack linkage (shared page) | server S-stack pool | kernel-mediated return trap | conventional |
| Mach migrating RPC (Draves 1991; Ford/Lepreau 1994) [M] | yes | thread struct + kernel continuation record | server-provided pool | reply migrates thread back | per-CPU stack cache via continuations |
| Spring shuttles (Sun 1993) [M] | yes | shuttle state + nucleus-recorded caller info | kernel-managed per-shuttle stack | door return | single-threaded nucleus, half-trap fast path |
| KeyKOS/EROS [V] | processor migrates; the callee runs its own saved context — call/return with a first-class transferable resume key, not same-thread migration | caller's state in its own domain object; the linkage is the resume key held by the callee | callee's own stack (part of domain state) | invoke the resume key (single-use, all copies nulled) | atomic kernel (EROS/Coyotos line) |
| Coyotos [V] | initially yes; abandoned for FCRBs — "no capability type analogous to a resume capability" | FCRB | callee's stack | FCRB reply | atomic kernel: deferred IRQs, at most one IRQ frame, stubborn locks |
| seL4 MCS [V] | no — scheduling-context donation gives migration's benefit without moving the thread | blocked TCB + reply object | server's own stack | reply-object invocation | non-blocking, per-core |
| L4 V2 long IPC [M] | no (handoff); kernel string copies faulted mid-copy — removed in V4 | TCB | n/a | reply rendezvous | per-thread kernel stacks |
| Solaris doors [V] | no — pooled server threads | client blocked in kernel | server thread pool | door_return | conventional |
| K42/Tornado [V] | migration semantics via per-call worker-thread handoff from per-(server,processor) pools | caller's Process Descriptor (PC+SP only; user saves the rest on its own stack) | kernel-mapped stack page from a per-processor pool, TLB-preloaded, recycled; permanent stacks opt-in per port | return trap with no register save; worker/stack/IN page returned to pools; resume after the call site | exception-level fast path: no locks, no shared data, per-processor replication |
| Opal [V] | yes at the design level — the thread transfers control through a portal (prototype ran it as Mach RPC) | kernel keeps the thread (prototype: Mach blocking-RPC state) | SAS: stack addresses stay valid; entry at a fixed global VA with the domain's GP register | procedure return through the portal; check fields validated at call time | Mach-hosted prototype |
| Nemesis [V] | no — deliberately anti-tunnelling: domain activations, event channels, shared buffers | blocked client thread's context in the domain's DCB context slots | server's own thread on its own stack; activation upcalls on a dedicated DCB stack | server sends an event and blocks; the kernel re-activates the client domain — every crossing pays the scheduler | no kernel threads; per-processor fixed kernel stack; 12-instruction first-level ISRs |
| NOVA [V] | donation — a linked EC chain; the callee's time flows from the caller's scheduling context | `Exc_regs` embedded in the Ec object (kernel memory, written in place via TSS RSP0); kernel continuations are function pointers | destination EC's own user stack, fixed at EC creation; the kernel sets only the IP per call — the stub owns stack hygiene | REPLY syscall: UTCB copy + unlink + make-caller-current; the reply is an implicit per-call single slot in the callee EC; a double reply silently deschedules | one kernel stack per CPU since the 2010 era — an interrupt kernel |
| Clouds [M; bibliography V] | yes — the thread itself, with its stack, migrates into the passive object's address space, even across machines | the thread's own traveling stack (ordinary frames) | the thread carries its stack; the kernel maps it into the target | ordinary procedure return (migrate back, pop the frame) | Ra kernel replicated per node |
| Grasshopper [M; bibliography V] | yes — loci migrate between persistent domains | the locus's stack, itself a persistent object (durable across crashes) | the locus's own stack object mapped into the current domain | migrate back (pop the frame) | conventional, plus persistence machinery |
| Mungi PDX [V] | domain union, not switch — the call runs in the union of a caller-chosen subset and a fixed PDX domain; implemented as a spawned thread (blocking RPC over L4) | caller blocked (L4 RPC state); domain setup cached across calls | the spawned task's stack | `PdxReturn` re-establishes the caller's original domain | L4-hosted |
| Pebble portals [M] | yes | kernel-saved frame | target component's stack | portal return | conventional |

Survey warnings carried into the design: L4 long IPC — never let the PPC path perform kernel-mediated copies that can take user page faults mid-invocation (large payloads travel through pre-shared memory, matching the fbuf direction); Mach migrating threads — foreign-thread stack provisioning, accounting, and faults in migrated frames were the recurring pain, while the kernel-continuation discipline survived into every modern kernel (hence: continuation Thread-resident before any switch); KeyKOS — single-use resume keys (all copies nulled on first invocation) prevent double return, at the cost of kernel tracking of every copy; Coyotos — resume keys were abandoned when SMP demux and richer delivery were needed, so the Invocation kind must leave room for a rendezvous-style object without breaking the return ABI; seL4 MCS — donation achieves migration's scheduling benefit without moving the thread, so PPC's payoff must come from what donation cannot give (zero-copy register ABI, no per-server Threads, true call/return nesting). K42/Tornado also uses the name PPC (Protected Procedure Call) — a searchability collision to be aware of.

Cross-cutting from the second dig (2026-09-29): K42/Tornado shows that replicating every call-path resource per processor (no locks, no shared data) is what makes an exception-level fast path compatible with cross-domain calls, with MetaPort redirect-and-replay keeping exhaustion and first-use off that path; Nemesis supplies the crosstalk argument — a migrated thread's CPU accounting must stay with the client and the caller's scheduling entity must remain runnable (K42's dispatcher-stays-runnable is exactly that fix); NOVA is the only system found combining all four of vesper's selected directions and has run that way since 2010, with helping as its busy-callee policy and per-vector exception portals (keeper-editable, MTD-bounded fault state) as its fault-delivery answer.

#### KeyKOS invocation model (verified)

Verified from the archived KeyKOS Architecture documentation (agorics.com, via the Internet Archive): `architecture/Domain.States.html`, `architecture/Intro.Domains.html`, `architecture/Messages.html`, `architecture/Three.Special.html`.

- A domain is the actor: sixteen general key slots plus special slots. The address slot holds the key to its address segment (its address space); the general registers, floating registers, and status live in its special slots as degenerate number keys — the full execution state is stored in the domain object itself.
- A domain is always in one of three states — running, available, or waiting — and the state transition is determined solely by the invocation type, never by which key is invoked.
- Two kinds of gate keys: a start key delivers only when the designated domain is available (otherwise the invoker queues — a domain is a serially reusable resource, so the kernel never allocates per-call stack frames or copes with storage exhaustion; reentrant services are factories that create a fresh domain per request). A resume key exists only to a waiting domain and is created by CALL.
- Three invocation instructions: FORK leaves the invoker running (asynchronous); CALL leaves the invoker waiting and implicitly appends a resume key to the invoker as the last of the message's four keys (a message is a parameter word, up to a 4096-byte string, and four keys; the kernel does not buffer messages); RETURN leaves the invoker available and promptly dequeues one queued caller.
- The return path is a first-class key, not kernel-internal state: the dynamic calling/called relationship "is represented by a key first held by the called domain. Most other capability systems represent this relationship in some sort of internal stack... the CALL operation, which is primitive, produces a message that contains an implicitly produced resume key to the CALLing domain... it may be passed to another domain, stored into an array of keys or anything else that may be done with a key."
- Single use is enforced globally: "as a resume key is invoked, all resume keys to the designated domain disappear and are everywhere efficiently replaced by null keys" — double return is impossible even with copies; the kernel tracks every copy to null them simultaneously.
- CALLing a resume key is co-routine linkage: the two domains swap states and each CALL mints a fresh resume key back, so producer/consumer streams need no buffering.
- There is no kernel invocation stack: the call chain is distributed across waiting domains, each holding its own saved state, linked by resume keys held as ordinary keys; nesting depth is bounded by the number of domains.
- Faults reuse the mechanism: a domain, segment, or meter keeper is CALLed with a service key and a resume key to the faulting domain, and resumes it by RETURN — the faulting domain never notices the interruption. Meters form the CPU-accounting chain; the meter keeper is CALLed with a resume key to the domain that exhausted its budget, so scheduling policy lives in domain code.

Consequences for the return-operation decision (resolved 2026-09-29 in favor of a fixed well-known return key): KeyKOS is the canonical minted-return-capability design. It buys deferred return, delegated return (whoever holds the resume key completes the call), co-routine streaming, and fault delivery through the same primitive. It costs: kernel tracking of every resume-key copy to enforce single use; a serialized callee (no reentrancy — concurrency via domain factories); and no cheap LIFO pop — every return is a domain wakeup. Under vesper's selected kernel-internal invocation-stack direction, the deferred/delegated-return and co-routine patterns compose in userspace over Notification/EventCount instead. One gap to carry into D1's open fault-handling work: KeyKOS/EROS deliver a fault as a CALL carrying a resume key to the faulting domain; a fault in a migrated frame under the invocation-stack model has no transferable continuation, so keeper-style fault delivery needs its own answer (a Thread-control authority rather than a return key).

Vesper's selected `Thread.Return` on `CurrentReturnOnly`, combined with a context-redirecting return, is structurally single-use — without KeyKOS's copy tracking. `Return` rewrites the invoking Thread's saved PC/SP/AS to the popped record's context and returns to user mode there, so the callee's continuation is abandoned at the first `Return`: the callee's code never executes again and cannot issue a second one (two adjacent `Return` instructions are dead code after the redirect), and the pop consumes the top record, so no record is popped twice or out of order. Another Thread in the same AddressSpace may invoke the AS-shared sentinel after ordinary guard/incarnation/presence checks, but it can pop only its own current stack, never the original caller's; the key is not transferable per-invocation authority. A callee therefore holds exactly-once, return-to-immediate-caller authority by construction. This is the selected PPC contract, and the implemented Return path follows it. The only misuse left is a stray `Return` issued by the *resumed* domain's own code (corrupted control flow within one domain), which is indistinguishable from that domain returning early and grants it nothing it did not already have; the kernel-detectable cases are depth-zero underflow and target-AddressSpace liveness — retirement being the one external event that can invalidate a saved continuation without the Thread's cooperation.

#### Reference-implementation continuation and time-attribution mechanics (Composite, NOVA, seL4 MCS)

Continuation records, as built:

- **Composite**: `struct invstk_entry { struct comp_info comp_info; unsigned long sp, ip; unsigned long ulk_stkoff; prot_domain_t protdom; }` `HALF_CACHE_ALIGNED` (`thd.h`), `THD_INVSTK_MAXSZ 16`, `invstk[16]` plus a `u16 invstk_top` inline in the TCB; the top is cached per CPU (`cos_info->invstk_top`) and committed in `thd_current_update` (`prev->invstk_top = cos_info->invstk_top; …`), while preemption saves only `pt_regs` and touches no invstk entry. Push-full returns `-1` to the caller as a raw errno; pop-empty is a `0xDEADDEAD` sentinel with `ip = sp = 0`. The kernel writes only the callee's entry IP — the callee's SP comes from the callee's own stub (`custom_acquire_stack`) and is never validated. The token rides a register set by the stub at entry and read back at return.
- **seL4 MCS**: `reply_t { tcb_t *replyTCB; call_stack_t replyPrev, replyNext; word_t padding; }` — four words, 32 B, retyped from a caller-held untyped into the caller's capability table, so the kernel allocates nothing for it. The call chain is a doubly-linked list threaded through the SC itself (`sc->scReply` plus type/isHead-tagged `call_stack_t` links), which gives chain→SC in O(1) with no kernel-side stack. Single use is structural: `doReplyTransfer` begins with `if (reply->replyTCB == NULL || tsType(...) != BlockedOnReply) return;`, and mid-chain revocation is `reply_remove`. The Call copies *no registers at all* — the passive callee is its own TCB with its own saved `SP_EL0`/`ELR_EL1` and its own capability-table and virtual-address-space roots.
- **NOVA**: no per-call record — the donation is a `caller`/`callee` pointer pair per EC, the caller identity is a single implicit slot inside the callee, and the caller's SC stays current. Arguments move through a UTCB page copy bounded by the MTD (1–512 words). A double reply is not a defined error: it self-blocks the callee and leaks the donated SC out of the ready queue, an anti-pattern not to copy.

Time attribution, as built:

- **Composite**: a per-CPU `curr_tcap` plus a per-thread `tcap_res_t exec` accumulator; `sinv_call` touches no tcap state at all, so callee code running on the caller's thread is charged to the *thread's* tcap automatically. Consumption is quantum-granular; the hierarchy is a flat `delegations[16]` vector of `(tcap_uid, prio)`, refilled only by an explicit `tcap_delegate` syscall.
- **seL4 MCS**: `schedContext_donate` moves the caller's SC onto the passive callee TCB (`callee->tcbSchedContext = sc; caller->tcbSchedContext = NULL`), and charging is the three-step kernel discipline `updateTimestamp()` (accumulate on entry) → `commitTime()` (charge `ksCurSC`, zero, never roll back) → `switchSchedContext()` (`ksCurSC = ksCurThread->tcbSchedContext`). Budget exhaustion mid-call is a *timeout fault to the server's own handler*, which still holds the client's reply object, so the chain survives.
- **NOVA**: accounting happens only at dispatch (`current->used += now - current->last` in `Scheduler::schedule`), so mid-call time is charged at the next dispatch; 2010 made the LAPIC timer itself the budget deadline.

Two consequences for the vesper record. First, correct *charging* needs nothing per-call: because the payer follows the thread in all three designs, a migrated thread's callee work is charged correctly by construction (Composite) or by moving the cap (seL4). The maintainer's transition-stamp requirement — stamp the caller's consumed time and mark the record "callee on caller's behalf" — is a *per-call observability* extension that no reference implementation has; it costs one 8-byte timestamp per frame, and the "on behalf" marker is just the record (caller AS + depth) with the thread's own DCB as payer. Second, Composite associates a `captbl` with each component, shared by its threads; this supports context-dependent table selection, but does not impose one table per hardware address space (see the shared/merged-domain analysis above). Vesper's callee is the caller's own Thread, so PPC must switch translation context *and* keytable on the way in. With the selected one-keytable-per-AddressSpace model, the continuation's source AddressSpace identity suffices to recover the source keytable on return; there is no separate per-frame keytable field. Vesper implements AddressSpace-associated table selection with the immutable issued carved binding described in the audit above. The per-Thread table field is removed, and the shared-AS dispatch regression proves two Threads use one table without reinstalling grants. Future table reclamation and full backing protection remain D3 work; the hardware migration trial is not covered by identity-only dispatch tests.

**Vesper selected record:** see the contract's [invocation stack](nucleus_capabilities.md#invocation-stack).

#### K42/Tornado PPC (verified)

Sources: Gamsa's Tornado thesis (Ch. 5), the Tornado OSDI'99 paper, the Hurricane-era ICPP'94 paper, the K42 EuroSys'06 and IBM SJ 44(2) papers, and the K42 source mirror.

- The model is migration semantics implemented as worker-thread handoff: "we chose to implement the PPC by creating a new process in the callee protection domain to handle each call" — for exception isolation, stack security, and process-model fit. The kernel saves only PC+SP in the caller's Process Descriptor; user level saves the remaining registers on its own stack before trapping.
- The fast path runs at exception level — no preemption, no locks, no shared virtual memory — so every resource it needs is replicated per processor: a PortID directly indexes a per-processor array of PortAnnexes, each holding that server's free worker pool on that processor; stack pages come from a per-processor physical pool, are TLB-preloaded, and are recycled between servers for cache warmth (with the explicit warning about untrusted servers sharing uncleared stacks; permanent stacks opt-in per port). Fixed 8KB stacks.
- Parameters: up to eight register words pass for free; beyond that, page-size IN/OUT pages are remapped, not copied — nesting works because each level uses its own OUT page, with an IN/OUT swap optimization for pass-through chains.
- Return is a dedicated trap with no register save (the worker is finished): unlink, transfer the callee's IN page back to the caller's OUT page, return worker/stack/page to their pools, resume after the call site. Slow paths (empty port, first use, remote redirect) are indistinguishable from the fast path and redirect to a MetaPort with pre-reserved resources that creates the missing worker and replays the call — keeping 133–227 µs first-use costs off the ~4.6 µs fast path.
- Unification: interrupt dispatching, upcalls, and process creation are all PPC variants — "all processes, even the first process in a program, arise due to a PPC call."
- K42 refinements: the PPC page — a per-processor physical page mapped read-write at a well-known virtual address into every process, treated as a register extension and preserved caller→callee→back; and "a thread making a PPC is blocked until the PPC returns, but its dispatcher remains runnable, allowing it to regain control and run other threads" — the Nemesis crosstalk fix.
- Numbers (150 MHz R4400): call 377 instructions / 695 cycles; core ≈ 167 instructions, close to two one-way L4 calls on the same hardware.
- Corrections to the earlier memory row: no "link tables" or "communal segments" exist in any reachable source (linkage records are LRPC's); the real structures are per-processor PortAnnex / call-descriptor / PPC-bak-page pools. Tornado's PPC is worker handoff, not literal migration, and K42 is not a single-address-space system.

#### Opal portals (verified)

Source: the Opal TOCS'94 paper (UW TR 93-04-02).

- A portal is an entry point to a domain named by a plain 64-bit portalID that "can be freely passed between domains, and so anyone can try to call through a portal"; the thread transfers control into the domain, beginning at a global virtual address fixed by the portal's creator, with the domain's GP register initialized. Capabilities are 256-bit values (portalID + object address + randomized check field); servers multiplex objects through one portal, validate check fields at call time, and can deny access even through a valid capability.
- Protected procedure call is the only way to cause code to execute in a child domain — there is no notion of "executing a program". Nesting and mutual calls are expected.
- The unique offer: naming decoupled from protection — globally meaningful, freely passable entry names with server-side call-time validation, instead of unforgeable kernel-minted entry capabilities. The cost: every portal is a DoS target and validation sits on the callee's critical path.
- Caution: the prototype had no native portals — it simulated them with Mach messages (133 µs cross-domain call versus 88 µs native Mach) — so its numbers must not be read as SAS-portal performance.

#### Nemesis (verified)

Sources: Roscoe's thesis (UCAM-CL-TR-376), the JSAC'96 paper, Barham's thesis (UCAM-CL-TR-403).

- Nemesis explicitly rejects thread tunnelling: "since the thread has left the client domain, it has the same effect as having blocked as far as the client is concerned. All threads must now be scheduled by the kernel... Accounting information must be tied to kernel threads, leading to the crosstalk" — the coupling of data transfer and control transfer "can seriously impede application-specific scheduling."
- Its alternative: domains are scheduled, not called. The kernel "consists almost entirely of interrupt and trap handlers; there are no kernel threads"; a system call builds a kernel stack frame in a fixed per-processor area. A domain gets DCB context slots (32 on Alpha) and is entered by an activation upcall on a dedicated DCB stack delivering time, deschedule reason, and slot index — usually into the domain's user-level scheduler. IPC is event channels plus shared buffers mapped read-only into the peer; the server runs its own thread on its own stack; every crossing pays the scheduler (null RPC ~30 µs, ~14 µs highly optimized).
- The x86 call-gate/LDT story from the earlier memory row could not be confirmed and is partially contradicted: primary sources describe Alpha/MIPS/ARM implementations with a single-DTB-flush domain switch. Treat that memory as wrong.
- Offers: the crosstalk argument as a design obligation (accounting follows the client; the caller's scheduling entity must stay runnable — K42's dispatcher split is the fix); a kernel shape independently convergent with the interrupt kernel (no kernel threads, per-processor fixed kernel stack, 12-instruction first-level ISRs delivering events into domains); gatekeeper heaps for revocable cross-domain sharing; and the structural alternative of pushing service code into client domains to avoid chains.

#### NOVA (verified)

Sources: the NOVA source (`github.com/udosteinberg/NOVA`, current release branch and the 2010-05-12 paper-era commit) and the archived NUL userspace documentation; the EuroSys'10 paper was not fetchable as text.

- The closest living relative of the selected PPC directions, and an existence proof: NOVA has run since the paper era with one kernel execution stack per CPU, the user frame embedded in the Ec object (trap entry writes it in place via TSS RSP0), and function-pointer kernel continuations — an interrupt kernel in vesper's sense.
- A portal call is a rendezvous: validate the portal capability (CALL permission, same CPU, destination EC free), link the ECs into a donation chain, set only the destination's IP (plus portal id and transfer descriptor), copy MTD-bounded message words between the kernel-allocated UTCBs, and make the callee current. Nothing else moves — each EC keeps its frame, UTCB, PD, and stack. "Donation" means the caller's scheduling context still funds execution: the scheduler walks the donation chain to find whose head to run.
- The destination's user SP is a creation-time parameter, validated once; the kernel does not touch it per call — the userspace portal stub owns stack hygiene. Vesper's planned call-time stack validation is strictly stronger; NOVA shows the weaker check suffices in practice.
- Return is the REPLY syscall: copy MTD-bounded words back, unlink, make the caller current; REPLY never returns to the callee. The reply is an implicit per-call single slot inside the callee EC (2010: a `Capability reply` field with a DISABLE_REPLYCAP call flag; today a plain `caller` pointer) — not user-transferable in the verified code. A double reply finds no caller and silently deschedules the callee.
- A busy callee means no queuing: the caller helps — it parks with a retry continuation and either activates the callee (running the callee's own chain: nested donation) or parks its scheduling context on the callee's wakeup queue; a livelock detector (100 helps in 2010; preemption points today) bounds the loop.
- Faults reuse the same mechanism as per-vector capabilities: a user-mode fault with vector V invokes the capability at `evt + V`; the faulting EC donates itself to the keeper, whose UTCB receives the faulting register state (transfer bounded by the portal's MTD); the keeper's REPLY writes its possibly-modified state back and resumes the faulting EC. The strongest found answer to the open migrated-frame fault-delivery question.
- The EC/SC split makes "whose time does a donated call charge" an explicit, separate decision; capability transfer moved out of the call path into a separate synchronous take-grant syscall in current code; EC death propagates up the chain (the caller resumes with an error, or is killed if itself in a fault chain). NOVA now also runs on AArch64, and a formal-verification branch exists.

#### Clouds and Grasshopper (bibliography verified; mechanisms from memory)

- Clouds (Georgia Tech; Computing Systems 1989, IEEE Computer 1991): the pure "thread migrates to the data" model — objects are passive, a thread migrates into the object's address space to invoke an operation, carrying its own stack, which the kernel maps into the target; nested invocations deepen the one traveling stack; distribution extends this across machines. Negative lessons: carrying the caller's stack into the callee couples the domains (the callee can read caller stack contents), and nesting is unbounded.
- Grasshopper (Sydney/Macquarie — not UNSW; Computing Systems 1994, POS-6 1994, CACM 1996): loci migrate between persistent domains; the locus's stack is itself a persistent object, so the cross-domain call chain is durable across crashes — the only system in this survey where continuations survive failure. The counter-lesson for a non-persistent kernel: durable call chains drag in checkpointing and a decade of consistency machinery; keep the invocation stack kernel-owned, bounded, and forgettable.

#### Mungi PDX (verified)

Source: the IWOOS'96 PDX paper.

- Protection Domain Extension is domain union, not domain switch: a PDX call executes in the union of a caller-chosen subset of the caller's domain and a fixed domain associated with the procedure; on return the caller's original domain is re-established. Conceptually similar to IBM System/38 profile adoption. Implemented as a spawned thread doing blocking RPC over L4, with the domain setup cached across calls.
- Offers: "callee gains a validated subset of the caller's rights for the duration of the call" as an alternative to capability-passing and to full domain switches; and kernel-invoked untrusted handlers via an empty capability list — a pattern for fault handlers.

#### Hardware precedent: Multics gates and VMS change_mode (verified)

- Multics originated the call gate: a gate segment with a vector of entries allowing controlled ring transfers, checked against per-segment ring brackets — three numbers (the highest ring that can write, read, and call-as-gate), making entry permission a distinct right from execute; each ring of each process has its own stack segment; the 645 simulated gates in software (the gatekeeper) before the 6180 did them in hardware.
- VMS `change_mode` is the same shape: CHMx dispatches through SCB-indexed vectors to sanctioned entry points, each access mode has static per-process stacks, and previous-mode bits govern the return.
- These are the hardware originals of "same thread crosses protection with a switched stack"; ring brackets are a compact precedent for separating entry permission from execute permission on an exported procedure.

## 8. Memory reclamation and protection

### Current code

The [`Frame` handler](../kernel/nucleus/src/api/arch/frame.rs) installs and withdraws real page/block descriptors through the target AddressSpace's tables; the frame's inline payload records the mapping identity (owning AddressSpace pool index and generation, full virtual address). The transactional [`Untyped` retype handler](../kernel/nucleus/src/api/untyped.rs) commits its watermark last. There is no Buffer handler: Buffer is a userspace/libOS construct. Remaining gaps: Frames have no lifetime generation of their own, a Frame records at most one mapping, and there is no teardown bookkeeping for partial map/unmap failures.

**Recommendation:** durable mapping identity, transactional creation, and retained partial-teardown bookkeeping. Before reuse, complete required CPU TLB/device translation synchronization and sanitize fresh ordinary RAM crossing protection boundaries. Intentional content-preserving sharing and device memory require distinct treatment.

The shared-address-space protection decision matters here: capability checks on syscalls cannot mediate arbitrary CPU loads/stores through installed translations. A common address namespace also does not imply that only one CPU can cache a translation.

### CONFIRMED Frame mapping direction

Recorded in the contract's [mapping and sharing](nucleus_capabilities.md#mapping-and-sharing): Copy is not Map, Unmap is mapping-local, origin Revoke withdraws descendants, remap is origin-authorized, and Frame slots are not reused after deprovisioning.

Open work for this section is tracked in [Phase 5](capabilities_implementation_plan.md#phase-5--memory-and-safe-reclamation); open decisions are in [`followup.md`](followup.md).

### D1: selected protection and sharing architecture

Recorded in the contract's [protection and system composition](nucleus_capabilities.md#protection-and-system-composition) and [decision register](nucleus_capabilities.md#decision-register).

SPeCK-like userspace policy over kernel liveness/resource/quiescence mechanisms remains the selected architecture. Possession of a permission authorizes its specified operation; it does not imply that its arbitrary syscall inputs are safe, that it follows fbuf protocols, or that it may violate protection of unrelated objects. Applications can deliberately damage resources they are permitted to write/retire; such delegated power is not a confinement escape.

### D1 clarifications and consistency checks

**1. Read-only mapping versus immutable backing — technical correction.** The inability of A to store through its read-only PTE does not prevent B's writable PTE from changing the same frame. The read-only DCB view is also intentionally updated by the kernel. Consequently, immutable sharing requires more than read-only permissions at one observer. The record preserves the hardware write-protection requirement without adopting the incorrect inference of global immutability.

**2. CONFIRMED** ([contract](nucleus_capabilities.md#d1-follow-up-decisions-and-remaining-questions)): fbuf setup agrees addresses for every participant before mapping and publishes pointers only afterwards.

**Scope decision:** multi-node global/distributed address allocation is out of scope. Do not import a classical SASOS assumption that one coordinated 64-bit namespace covers local RAM, neighboring nodes' RAM, and allocated disk space. The maintainer has raised concerns about global reservation; the exact machine-local reservation/allocation model remains open rather than being silently replaced by a specific scheme. The current concrete requirement is participant-compatible fbuf addresses established before mapping.

**3. Stale raw pointers after VA reuse** — see the contract's [protection requirements and boundaries](nucleus_capabilities.md#protection-requirements-and-boundaries).

**4. Unsafe Rust versus fault containment.** Unsafe shared-memory APIs can place validity, exclusivity, and synchronization obligations on their callers. They cannot promise ordinary Rust reference soundness if those obligations are violated; an eventual protection fault does not repair undefined behavior or compiler assumptions. That is an application/protocol error, but the nucleus must remain memory-safe and preserve confinement from malicious native code. Fbuf APIs need not expose ordinary references when the full-borrow guarantee is unavailable.

**5. Hardware breadth — features are not uniform.** Separate translation contexts are an acceptable fallback, but MPU-only targets may isolate regions without supporting arbitrary virtual aliases or page-table-style remapping. Per-target support restrictions and whether a requested mapping mode is unsupported must be explicit. Software-mediated DMA requires exclusive control of programming paths/descriptors; giving an untrusted Thread raw device-programming authority can bypass mediation. This document does not claim all listed machines currently support the same features.

**6. Availability policy versus resource enforcement.** Keeping recovery/admission policy in the libOS is consistent with the model. A malicious Thread may bypass its libOS, so the nucleus still must bound/charge its resource consumption and provide checked IPC/syscall failure rather than unbounded allocations or panics. Exact budget/quota and bounded-work mechanisms remain implementation work, not a new kernel scheduling-policy mandate.

**7. CONFIRMED** ([contract](nucleus_capabilities.md#lifecycle-and-authority-contract)): direct derivation authority entails bookkeeping responsibility and composition-scoped TCB membership.

### D1 implementation and validation work

### seL4 and Composite research: mapping authority versus object lifetime

seL4 evidence is pinned to **16.0.0, AArch64** where implementation-specific. Composite evidence distinguishes the older memory-manager interface from SPeCK/current kernel mechanisms. These comparisons inform Vesper; they do not override the confirmed semantics above.

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

**Lesson for Vesper:** Vesper adopts Copy-not-Map; broad origin withdrawal means cap-Revoke, not Page_Unmap. seL4 remains useful for per-cap mapping identity and the local-Unmap/origin-Revoke distinction. Its kernel-managed derivation, automatic deletion cleanup, final-cap lifetime rules, and remap authority need not be adopted: Vesper selects SPeCK-like manager/mechanism separation, permission-based object retirement, and accepted leaks.

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

The old SPeCK/PARSEC/C3 papers do not imply abandonment. The [official repository](https://github.com/gwsystems/composite) is not archived, and the already-inspected source pin [`3ef8f8c4d3296624640e6f3bd00801054d8350a3`](https://github.com/gwsystems/composite/commit/3ef8f8c4d3296624640e6f3bd00801054d8350a3) is a substantive merge dated **2026-08-31**. [PR #502](https://github.com/gwsystems/composite/pull/502) integrates generic guest-image builds, VMM I/O changes, and a networking-thread placement fix. [PR #498](https://github.com/gwsystems/composite/pull/498), merged **2026-02-18**, integrates Patina and VMX updates. These establish recent research development, not production support.

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

## 9. Complexity of remaining work

Relative engineering estimates include focused tests. They are not measured performance claims or schedule commitments; scope restrictions and protection/multicore choices can substantially change them.

| Work | Complexity | Main cost |
|---|---|---|
| Retirement of the remaining kinds and scheduler-shared records | Large | Shared-memory publication and cross-subsystem identity |
| General selective revoke plus memory reclamation | Very large | Descendants, mappings, hardware, bounded completion |
| Safe revocable mapping abstractions / multicore Time | Very large | Protection and concurrency contracts |

Indexed capability/object identity validation can be constant-time in steady state; retiring the shared object does not inherently require an ancestry walk. Full reclamation scales with affected mappings, pending operations, and cleanup records. Recursive manager unmapping can cost O(number of affected mappings), independently of later background cap-slot cleanup. Initial serialization can simplify the implementation without erasing those responsibilities. The kernel is not required to detect or recover allocation leaks.

Open work for this section is tracked in the [implementation plan](capabilities_implementation_plan.md); open decisions are in [`followup.md`](followup.md).

## Reachability boundary

Per-kind operation status is the catalogue in [`object_types/README.md`](object_types/README.md#catalogue); recheck dispatch in [`api/mod.rs`](../kernel/nucleus/src/api/mod.rs) before acting on it.

No implementation task or architectural decision is completed merely by documenting it.

**Summary recommendation:** keep userspace handles thin; put identity validation, authority enforcement, and retirement in the kernel. Add userspace ownership machinery where it protects slot management, transactional consumption, or direct memory access—not to mirror object liveness everywhere.
