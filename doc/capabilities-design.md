# Capabilities design

Why the [capabilities contract](capabilities-contract.md) says what it says: the problems each rule answers, the alternatives weighed, and the consequences for implementation. Prior-art evidence is in [capabilities research](capabilities-research.md); open work is in the [implementation plan](capabilities-implementation-plan.md) and open questions in [capabilities decisions](capabilities-decisions.md).

- **CONFIRMED** marks a maintainer-selected rule, stated normatively in the contract and linked from here. **INTERIM** marks a temporary direction whose enforcement still needs decisions.
- **Recommendation** marks a proposed direction, not an accepted decision. Alternatives are kept for context, not as competing instructions.
- **OPEN / DECISION REQUIRED** marks a question that must be settled before dependent implementation.

When a decision is approved, record it in the contract first, then reconcile this document and the plan.

## Working hypothesis: thin handles, authoritative kernel checks

The motivating position is that userspace representations should not closely track kernel object lifecycle: after an object is disposed, invoking its userspace capability should return an error.

**CONFIRMED** ([contract](capabilities-contract.md#vocabulary-and-identity)): handles name the capability incarnation obtained by the caller and need not monitor object liveness. The remaining qualification concerns mapped-memory APIs and precise completion contracts:

> Userspace handles need not mirror liveness, provided the kernel validates the intended identity and authority on each invocation. Direct memory access and ownership-consuming operations need additional contracts.

Not tracking whether an object is alive is different from not knowing which incarnation of an object or slot a handle names. A userspace liveness query is at most an observation; it cannot replace validation at the operation's authorization/commit boundary.

The desired separation is:

- A **handle** names the particular capability incarnation obtained by the caller, never an unrelated future occupant of its slot.
- A **capability entry** carries authority over a resource or delegation scope.
- A **resource** has independently managed identity, state, and physical lifetime.
- An **in-flight operation or mapping** can retain dependencies after its initiating handle disappears.

Phantom types improve ergonomics, not authority or lifetime enforcement. Copying a Rust handle is not kernel capability derivation.

## 1. Stale handles and identity reuse

### Slot-only counterexample

The original [`Key<T>`](../libs/object/src/key.rs) contained only `KeySlot` and `PhantomData<T>`. Its slot-only transport allowed this counterexample:

1. Slot 12 contains Invocation A.
2. Userspace retains a key naming slot 12.
3. A's entry is deleted and the slot is reused for Invocation B.
4. The retained key invokes slot 12 and can operate on B.

Even a type check cannot distinguish two Invocations. This need not escalate authority across address spaces—B is already in the caller's table—but it can use replacement authority unintentionally. Thus stale invocation did not necessarily fail under the former slot-only representation.

### Alternatives and selected semantics

The incarnation guarantee is now selected; a slot-only current-occupant API is not the chosen contract. **Maintainer clarification:** ordinary keys are local to the current Thread's implicit KeyTable — the table associated with the Thread's current AddressSpace — and carry a guarded table-relative slot plus incarnation. Numeric key transfer alone conveys no authority; authorized derivation/installation produces a recipient-local key. **Follow-up:** initially use a 32-bit slot and 32-bit incarnation, without freezing the total key size permanently. If type is embedded later, its 8 bits come from the slot field (24 slot bits, 32 incarnation bits), not incarnation. Capacity is fixed for now through an easily changed constant; runtime resizing is outside initial scope. **Approved packing:** slot in low 32 bits and incarnation in high 32 bits, with no initial type tag. Typed handles are non-owning. Live Threads may not rebind to a fresh logical invocation table with reset counters. Kernel allocation references use pool identity/index and a 32-bit generation, with no generation wrap or metadata-identity reset; concrete ownership and Thread integration remain work. Independent shared-object lifetime validation remains required. Generation wrap must not silently resurrect stale identities; Move invalidates the source on commit and yields a destination-local key. The included ABI/bootstrap paths have migrated together without a slot-only fallback; excluded sketches remain unmigrated, unsupported design material.

| Choice | Consequences | Complexity |
|---|---|---|
| Slots name their current occupant, like file descriptors | Accept reuse hazards; userspace coordinates slot ownership | Low kernel complexity |
| Exclusive userspace slot allocator with owned slots and borrowed handles | Prevent accidental reuse while handles exist, provided every mutation follows that allocator | Medium; requires enforceable runtime discipline |
| Kernel-checked slot incarnation supplied with invocation | Retained handles fail after replacement without monitoring liveness | Medium cross-layer change, including ABI/bootstrap |

**CONFIRMED** ([contract](capabilities-contract.md#approved-key-identity-and-wire-package)): invoking an altered or invalidated incarnation returns an explicit inconsistency error, never a silent refresh against replacement authority.

Authorized resource-state changes are not automatically capability replacement. In particular, origin-authorized Frame remapping is a valid operation even when mapping location changes. **Selected initial scope (3A=A):** no in-place rights/badge mutation. Derive attenuated authority into another slot and optionally delete the original; CopyDerive preserves badges and rebadging is deferred. Future in-place mutation semantics are not a prerequisite for this initial slice.

**Selected exhaustion behavior (3B=A):** a slot unable to issue a fresh 32-bit incarnation is permanently unavailable for further installation within that table lifetime; existing-capability deletion remains possible and other slots remain usable. No silent wrap or whole-table retirement is implied. The approved key package starts unused counters at 0, issues 1 on first installation, advances only at successful installation, retains counters on deletion, and reports exhaustion with status 28. Replacing/rebinding the table must not reset stale-key protection.

**INTERIM** ([contract](capabilities-contract.md#mapping-and-sharing)): Frame deprovisioning fully revokes the capability and does not reuse that slot for Frames.

Three distinct identity problems must be covered:

- **Slot incarnation:** whether a local handle refers to a replaced entry.
- **Object/thread incarnation:** whether a kernel reference refers to recycled storage.
- **Selective delegation-scope validity:** relevant only if one branch of capabilities to a still-live object must be revoked while other branches survive.

Derived capabilities reference the same kernel object. An authoritative object-generation mismatch is sufficient to reject all old invocations of that retired object, without walking a capability ancestry chain. The branch distinction matters for selective revocation, not for detecting global object retirement. A generation is not a secret or a substitute for authorization, and object generation alone does not detect replacement of a slot with a different valid capability.

Open work for this section is tracked in [Phase 4](capabilities-implementation-plan.md#phase-4--capability-storage-and-thread-lifetime); open decisions are in [`capabilities-decisions.md`](capabilities-decisions.md).

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

Recorded in the contract's [lifecycle and authority contract](capabilities-contract.md#lifecycle-and-authority-contract): Retype-origin capabilities carry delegable lifetime-control permission, the creator keeps control after donation, kernel retirement is separate from manager subtree cleanup, and abandoned allocations may leak without kernel recovery. Two consequences: capability reference counting alone would not account for pending invocations, mappings or internal relationships; and a pin that keeps storage safe is **not** permission to keep using revoked authority — physical lifetime and logical authorization stay separate.

Open work for this section is tracked in [Phase 4](capabilities-implementation-plan.md#phase-4--capability-storage-and-thread-lifetime) and [Phase 5](capabilities-implementation-plan.md#phase-5--memory-and-safe-reclamation); open decisions are in [`capabilities-decisions.md`](capabilities-decisions.md).

## 3. Guarded kernel access and storage lifetime

### CONFIRMED: correctness before representation optimization

Recorded in the contract's [key tenets](capabilities-contract.md#key-tenets) (correctness before representation optimization); the skill requires the matching layout audit with every representation change.

Open work for this section is tracked in [Phase 4](capabilities-implementation-plan.md#phase-4--capability-storage-and-thread-lifetime); open decisions are in [`capabilities-decisions.md`](capabilities-decisions.md).

### Selected guarded-access direction and remaining representation work

**CONFIRMED** ([contract](capabilities-contract.md#selected-local-key-and-storage-foundation)): single-core kernel execution with SMP deferred, short-lived guarded references from an owning access context, and all managed storage from accounted Untypeds.

Typed pools remain a useful starting point, with representation free to change under the correctness-first rule:

1. Persistent capabilities naming pooled objects carry checked object identities, conceptually `(pool, index, generation)`. The explicit `CurrentReturnOnly` Thread selector names no concrete object and rejects object-identity extraction.
2. Authoritative allocation metadata records allocation, generation, and retirement state, with its own valid backing lifetime.
3. An owning access context resolves identities and provides short-lived guarded access.
4. Multi-object operations resolve same-table/same-object aliases explicitly.
5. Object references and incompatible guards end before scheduling or context switching.
6. Pending operations retain checked identities and explicit reservations, not borrowed Rust references.

**SELECTED — D3 concrete guarded access:** capabilities naming pooled objects store only a checked object identity (pool tag, pool index, allocation generation); entries hold no raw object pointer. Named Thread capabilities use `ThreadSelector::Named(ObjectId)`; the separate `ThreadSelector::CurrentReturnOnly` selector has no concrete Thread identity to resolve and `object_id` extraction returns `InvalidOperation`. It still requires ordinary checked caller-table/key lookup, not a fabricated zero identity or lookup bypass. The owning access context computes object addresses from pool bases after validating per-pool authoritative slot metadata (allocation state + generation + retirement), whose backing is stable and independent of object storage — validation always precedes dereference. The context is constructed once per invocation under the kernel lock, is `!Send`, and hands out guards that borrow it, so the borrow checker enforces guards ending before scheduling. Multi-operand operations use explicit pair-resolution forms that reject aliased mutable operands up front; a lock serializes executions but does not by itself prevent two operands within one execution from naming the same object. Mutable resolution exclusively borrows the pool for the invocation, so the borrow checker prevents holding two mutable guards into one pool; clients requiring two objects from the same pool must request them through the pair-resolution API (`resolve_pair_mut`) rather than sequential mutable resolution. One pool per type tag is the invariant making this sufficient; introducing multiple pools per type requires revisiting cross-pool alias enforcement (for example a runtime alias set). Single-core remains selected; SMP is not an open choice. **Remaining D3 work:** Untyped-backed pool ownership, kernel-private KeyTable backing, and safe pool-backing lifetime (Phase 5, with D1/D6 for protection bindings); thread lifecycle integration; per-kind access details. The boot Untyped allocation and the shared `RegionPayload::reserve` watermark-allocation primitive are established (Kickstart's `carve_region` and `ObjectPool::carve` carve the initial `Nucleus` and all six pools from the boot Untyped's unused watermark range; runtime Retype reserves through the same primitive); syscall entry resolves the caller's table through the `Access` context. The boot table is carved kernel-privately by Kickstart, runtime `Untyped.Retype` creates further tables from an Untyped's unused watermark range, and carved-table resolution uses address-guarded `resolve_carved` forms with alias rejection. Retired pool slots stay `Retired` until manager cleanup completes ([contract](capabilities-contract.md#lifecycle-and-authority-contract)); the release handshake and reuse validation remain.

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

**CONFIRMED** ([contract](capabilities-contract.md#authorization)): table management follows ordinary source/destination KeyTable permissions with no manager-identity exceptions; Kickstart establishes the first Untypeds and tables, then hands authority on.

Open work for this section is tracked in [Phase 4](capabilities-implementation-plan.md#phase-4--capability-storage-and-thread-lifetime); open decisions are in [`capabilities-decisions.md`](capabilities-decisions.md).

### Who tracks revocation?

**CONFIRMED** ([contract](capabilities-contract.md#lifecycle-and-authority-contract)): components granted management rights derive, install and manage capabilities directly and own the bookkeeping, joining that composition's TCB; multiple or hierarchical managers are policy.

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

“Direct references” means `&[u8]`, `&mut [u8]`, or a reference into a shared record, produced by userspace wrappers after mapping—not raw pointers granting authority across the kernel boundary. The control interface remains capability-based; subsequent loads/stores are not capability invocations and cannot return the inconsistency status.

**`MappedSlice` ownership design:** create and own a private capability/mapping inside the guard, allow no further derivations or independently usable management aliases, and release the mapping on Drop.

**CONFIRMED** ([contract](capabilities-contract.md#allocation-and-representation)): Drop cleanup uses an incarnation-bearing key, so a stale key is rejected rather than unmapping a replacement.

There are two different exclusivity questions: ownership of the capability/mapping, and access to the physical bytes. No derivations from the guard-owned capability addresses the first. It does not prove that the physical frame has no aliases in other AddressSpaces, ancestor retirement authority, or device writers. D1 now prohibits multiple virtual aliases within one AddressSpace, but permits cross-AddressSpace writable sharing.

**CONFIRMED** ([contract](capabilities-contract.md#protection-requirements-and-boundaries)): authorized revocation is not vetoed by a client's Rust borrow; access after withdrawal to a still-unmapped address faults.

**Preferred Rust direction:** explicit unsafe caller obligations for reference-producing access, because the kernel cannot generally promise cross-AddressSpace exclusivity of shared resources. This is not a decision to make every mapping operation unsafe. The exact unsafe API and its full-borrow obligations remain to be designed; raw/fbuf protocol access may be preferable where an ordinary reference cannot be justified. Safe wrappers may exist only where they actually enforce stronger conditions. The kernel must remain memory-safe and isolate unrelated authority even if the application violates its unsafe contract.

**Technical correction — read-only is not immutable backing:** read-only PTE permissions prevent stores through that particular mapping. For example, A can map frame F read-only while B maps F read-write; B's permitted write changes the bytes A observes. DMA and kernel writers can also change them. This is consistent with hardware read-only protection, but inconsistent with calling the backing immutable. A read-only DCB mapping updated by the kernel is an existing example. Ordinary `&[u8]` needs no conflicting mutation for its borrow, not merely an inability to write through A's PTE. An immutable-sharing mode must establish a stronger backing/protocol guarantee.

**Stale raw pointers after VA reuse:** the current rule is in the contract's [protection requirements and boundaries](capabilities-contract.md#protection-requirements-and-boundaries); stronger guarantees are [future work](capabilities-decisions.md#future-work).

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

Open work for this section is tracked in [Phase 5](capabilities-implementation-plan.md#phase-5--memory-and-safe-reclamation); open decisions are in [`capabilities-decisions.md`](capabilities-decisions.md).

## 6. Thread identity and scheduler records

The selected direction — scheduler-owned record pages shared through `Scheduler.ShareRegion`, the root scheduler established by Kickstart, root-only Thread creation and donation — is in the contract's [Thread and AddressSpace contracts](capabilities-contract.md#thread-and-addressspace-contracts). Composite's upcall-based scheduling and scheduler-owned thread structures (Parmer, TECS 2013) are design evidence, not a wire-contract source.

Open work for this section is tracked in [Phase 4](capabilities-implementation-plan.md#phase-4--capability-storage-and-thread-lifetime) (scheduler-shared records) and [Phase 7](capabilities-implementation-plan.md#phase-7--time-and-userspace-scheduling); open decisions are in [`capabilities-decisions.md`](capabilities-decisions.md).

## 7. Pending operations, PPC Invocation, and Time

For a blocked caller, “the next invocation fails” is inadequate: there may never be another invocation. Retirement must arrange an explicit continuation outcome. Revocation/timeout cannot undo already committed effects or imply that a delivered request was never processed.

### CONFIRMED vocabulary for aborted work

Recorded in the contract's [aborted-work vocabulary](capabilities-contract.md#aborted-work-vocabulary): rejected before admission, cancelled before commit, completed, outcome unknown; the nucleus provides local mechanisms only.

Open work for this section is tracked in [Phase 6](capabilities-implementation-plan.md#phase-6--deferred-completion-and-ipc) and [Phase 7](capabilities-implementation-plan.md#phase-7--time-and-userspace-scheduling); open decisions are in [`capabilities-decisions.md`](capabilities-decisions.md).

## 8. Memory reclamation and protection

**Recommendation:** durable mapping identity, transactional creation, and retained partial-teardown bookkeeping. Before reuse, complete required CPU TLB/device translation synchronization and sanitize fresh ordinary RAM crossing protection boundaries. Intentional content-preserving sharing and device memory require distinct treatment.

The shared-address-space protection decision matters here: capability checks on syscalls cannot mediate arbitrary CPU loads/stores through installed translations. A common address namespace also does not imply that only one CPU can cache a translation.

### CONFIRMED Frame mapping direction

Recorded in the contract's [mapping and sharing](capabilities-contract.md#mapping-and-sharing): Copy is not Map, Unmap is mapping-local, origin Revoke withdraws descendants, remap is origin-authorized, and Frame slots are not reused after deprovisioning.

Open work for this section is tracked in [Phase 5](capabilities-implementation-plan.md#phase-5--memory-and-safe-reclamation); open decisions are in [`capabilities-decisions.md`](capabilities-decisions.md).

### D1: selected protection and sharing architecture

Recorded in the contract's [protection and system composition](capabilities-contract.md#protection-and-system-composition) and [decision register](capabilities-contract.md#decision-register).

SPeCK-like userspace policy over kernel liveness/resource/quiescence mechanisms remains the selected architecture. Possession of a permission authorizes its specified operation; it does not imply that its arbitrary syscall inputs are safe, that it follows fbuf protocols, or that it may violate protection of unrelated objects. Applications can deliberately damage resources they are permitted to write/retire; such delegated power is not a confinement escape.

### D1 clarifications and consistency checks

**1. Read-only mapping versus immutable backing — technical correction.** The inability of A to store through its read-only PTE does not prevent B's writable PTE from changing the same frame. The read-only DCB view is also intentionally updated by the kernel. Consequently, immutable sharing requires more than read-only permissions at one observer. The record preserves the hardware write-protection requirement without adopting the incorrect inference of global immutability.

**2. CONFIRMED** ([contract](capabilities-contract.md#d1-follow-up-decisions-and-remaining-questions)): fbuf setup agrees addresses for every participant before mapping and publishes pointers only afterwards.

**Scope decision:** multi-node global/distributed address allocation is out of scope. Do not import a classical SASOS assumption that one coordinated 64-bit namespace covers local RAM, neighboring nodes' RAM, and allocated disk space. The maintainer has raised concerns about global reservation; the exact machine-local reservation/allocation model remains open rather than being silently replaced by a specific scheme. The current concrete requirement is participant-compatible fbuf addresses established before mapping.

**3. Stale raw pointers after VA reuse** — see the contract's [protection requirements and boundaries](capabilities-contract.md#protection-requirements-and-boundaries).

**4. Unsafe Rust versus fault containment.** Unsafe shared-memory APIs can place validity, exclusivity, and synchronization obligations on their callers. They cannot promise ordinary Rust reference soundness if those obligations are violated; an eventual protection fault does not repair undefined behavior or compiler assumptions. That is an application/protocol error, but the nucleus must remain memory-safe and preserve confinement from malicious native code. Fbuf APIs need not expose ordinary references when the full-borrow guarantee is unavailable.

**5. Hardware breadth — features are not uniform.** Separate translation contexts are an acceptable fallback, but MPU-only targets may isolate regions without supporting arbitrary virtual aliases or page-table-style remapping. Per-target support restrictions and whether a requested mapping mode is unsupported must be explicit. Software-mediated DMA requires exclusive control of programming paths/descriptors; giving an untrusted Thread raw device-programming authority can bypass mediation. This document does not claim all listed machines currently support the same features.

**6. Availability policy versus resource enforcement.** Keeping recovery/admission policy in the libOS is consistent with the model. A malicious Thread may bypass its libOS, so the nucleus still must bound/charge its resource consumption and provide checked IPC/syscall failure rather than unbounded allocations or panics. Exact budget/quota and bounded-work mechanisms remain implementation work, not a new kernel scheduling-policy mandate.

**7. CONFIRMED** ([contract](capabilities-contract.md#lifecycle-and-authority-contract)): direct derivation authority entails bookkeeping responsibility and composition-scoped TCB membership.

## Summary

**Summary recommendation:** keep userspace handles thin; put identity validation, authority enforcement, and retirement in the kernel. Add userspace ownership machinery where it protects slot management, transactional consumption, or direct memory access—not to mirror object liveness everywhere.

Indexed capability/object identity validation can be constant-time in steady state; retiring the shared object does not inherently require an ancestry walk. Full reclamation scales with affected mappings, pending operations, and cleanup records. Recursive manager unmapping can cost O(number of affected mappings), independently of later background cap-slot cleanup. Initial serialization can simplify the implementation without erasing those responsibilities. The kernel is not required to detect or recover allocation leaks.
