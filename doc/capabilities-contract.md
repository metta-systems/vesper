# Capabilities contract

## Status and authority

This is the implementation reference for Vesper's capability system across the shared/userspace interface, nucleus syscall API, kernel object state, and architecture backends. It records target contracts, not a claim that the current code implements them.

- **Contract** means an invariant or direction to preserve throughout implementation.
- **Baseline** means a documented existing convention to reconcile across layers, not proof of support or correctness.
- **Open decision** means a choice that must be settled before the dependent feature is implemented. Recommendations are not silently binding decisions.

When implementation and this document disagree, record the discrepancy and migrate the code deliberately. Do not silently change the contract to match a stub. Architectural changes require an explicit decision here and corresponding updates to the [implementation plan](capabilities-implementation-plan.md). This document states the current contract; its history is in version control.

## Key tenets

1. **Mechanism in the nucleus, policy in userspace.** Resource managers, schedulers, drivers, and applications receive only the authority they need. The nucleus enforces protection, accounting, and safe transitions; it does not choose general allocation or scheduling policy.
2. **Explicit authority, no ambient access.** Possessing a numeric address, domain ID, slot number, or typed Rust wrapper does not confer authority. Kernel-resident capabilities and validated delegation determine what a caller may do.
3. **One capability invocation model.** Object operations, including memory creation and time management, use `CapInvoke`. Ergonomic wrappers are not additional kernel primitives. This does not specify unrelated exception or boot entry points.
4. **Separate handles, capabilities, and resources.** A local slot handle names an entry; the entry carries authority; the resource has its own identity, lifetime, and state. These are not interchangeable.
5. **Allocation and temporal authority are accounted resources.** Memory-backed objects originate from authorized untyped memory and explicitly accounted metadata. Memory allocation alone cannot create CPU budget, an Invocation authority, an IRQ entitlement, or an available ASID.
6. **Monotonic delegation.** Derivation cannot amplify rights, extent, time budget, or other authority. Copies, moves, and revocation have explicit per-kind semantics.
7. **Revocation precedes safe reuse.** Retirement includes in-flight use, mappings, hardware state, and outstanding operations where relevant. Removing a slot or incrementing a generation is not automatically sufficient.
8. **No fabricated Rust safety.** Type tags and phantom types aid programming but do not prove lifetime, exclusivity, mapping validity, or protection from another domain/device. Safe APIs must establish those guarantees.
9. **Defined failure semantics.** Malformed requests return errors, not panics or truncated valid requests. Pre-commit failure preserves resources and ownership. Partial completion is explicit rather than reported as an ordinary all-or-nothing failure.
10. **Scheduler-owned shared records, kernel-private execution state.** A user-space scheduler explicitly shares scheduler-owned pages with the kernel for scheduling records. The scheduler can write every byte in pages it maps; kernel-only TCB/execution state remains separate and inaccessible to it. Scheduling policy is userspace-owned, while the kernel enforces transitions using its private state and the agreed record protocol.
11. **Protected invocation and complementary communication primitives.** `Invocation` is a Protected Procedure Call (PPC): it identifies a component API entry point in a target AddressSpace, and the source Thread migrates into that AddressSpace. Entry/return mechanics and call lifecycle remain open. Large payloads use shared memory; notifications coalesce event identities; event counts preserve progress. Do not collapse these distinct mechanisms into one primitive.
12. **Bounded and testable work.** Prefer explicit storage limits, typed pools, reserved IPC resources, and no hidden hot-path allocation. Long revocation or teardown work needs a bounded/incremental completion contract.
13. **Complete vertical slices.** An operation is supported only when its shared contract, userspace encoding/decoding, kernel authorization, object transitions, and tests agree. A file or enum variant is not evidence of support.
14. **Correctness before representation optimization.** Add, remove, or expand kernel-object and key fields as necessary for consistent identity, authority, mapping, and lifetime semantics; current sizes, packing, and inline representations are not optimization constraints to preserve. Optimize only in a later measured pass. Every layout change must also update affected allocation/accounting, pool capacities/alignment, strides, DCB page views, shared ABI consumers, and layout assertions/tests. Hardware formats and agreed wire encodings still require deliberate, coordinated treatment.

## Kernel success traces

Successful capability operations emit a green-check semihosting trace in the form `✅ Type::Operation()` after the state transition commits and before returning success. Failed/rejected operations do not emit a success trace. Keep new operation handlers consistent with this convention.

## Protection and system composition

### Selected D1 architecture

- **Threat model:** Threads may execute arbitrary native code, including deliberately malicious assembly, forged pointers, and incorrect unsafe code. Confinement must not depend on safe Rust, cooperative library wrappers, or the confined program obeying protocols. Planned validation includes adversarial agent tasks attempting to circumvent isolation.
- **Explicit trust:** KeyMaster can manipulate keys because it receives the corresponding capabilities, not because its name/identity bypasses authorization. Any application granted equivalent rights is entrusted with those operations within their scope; this is not blanket trust in its inputs or access to unrelated resources. Each AddressSpace remains independent. Derivation-manager policy and the kernel-enforced authority boundary must remain distinct.
- **Protection unit:** the `AddressSpace` is the protection-context boundary. Distinct AddressSpaces must not be treated as execution contexts sharing an unprotected memory context; a Thread executes in exactly one AddressSpace. Backend binding and lifecycle representation remain D1/D6 implementation work.
- **Single address space:** shared numerical meaning and cheap sharing are the goals, not a mandatory shared translation root or zero context-switch cost. When shared backing is properly mapped at X in both AddressSpaces, a pointer X can be passed unchanged. Shared fbufs should preferentially use such common mappings and higher-level synchronization. Fbuf setup must establish virtual addresses suitable for every participating AddressSpace before installing the mappings; shared pointers are published only after setup succeeds. Every shared pointer's pointee needs a corresponding authorized mapping; mapping the pointer-containing fbuf does not grant access to arbitrary addresses stored inside it.
- **Allowed mappings:** no physical frame may appear at two different virtual addresses within one AddressSpace. Across AddressSpaces, mapping the same frame at different addresses is allowed; the same address is preferred for fbufs/pointer sharing. Simultaneous writable mappings across AddressSpaces are permitted, with fbuf/higher-level protocols managing synchronization. Exclusive, immutable-shared, and mutable-shared resource modes are all supported design requirements; their transition protocols remain D3/D6/D7.
- **Fallback and portability:** use separate protected translation contexts, preserving shared-address conventions where possible. Target range is PowerPC G5 through current Intel, Armv9, and RISC-V machines, potentially higher-end STM32. This is intended portability, not a claim of implemented backends or equivalent MMU/MPU/IOMMU features on every target. A per-target protection/translation feature matrix and handling of unavailable features remain open.

### Protection requirements and boundaries

Other AddressSpaces' memory confidentiality/integrity must be preserved except for explicitly granted access; kernel-private memory confidentiality/integrity are always required. Everything except the nucleus runs in userspace by default, with no unconfined promoted services planned initially. Any future promotion would enlarge the trusted computing base and requires an explicit security decision, not an ordinary capability grant. Side-channel resistance is deferred.

DMA must be confined through a trusted mediation component or an IOMMU as appropriate to the platform. Software mediation is sufficient only if untrusted code cannot bypass it to program DMA-capable hardware/descriptors; an IOMMU path needs its own mapping and invalidation guarantees. Device-level implementation and unavailable-feature behavior remain D1/D6.

Availability/resource-exhaustion policy belongs to the libOS. The nucleus provides resource isolation/abstraction and IPC: it must preserve charged-resource bounds, checked failure behavior, and isolation even when a Thread bypasses its libOS. Exact resource/time accounting and bounded-work mechanisms remain their respective implementation decisions; this does not move general recovery/allocation policy into the kernel. Timing/cache side-channel resistance is deferred but must remain a design consideration; no side-channel confidentiality guarantee is claimed at this stage.

A shared namespace is not itself an isolation mechanism. MTE/PAC alone are not sufficient confinement for malicious native Threads. Each backend must enforce the selected protection boundary for ordinary loads/stores and privileged operations, independently of capability-invocation checks.

### D1 follow-up decisions and remaining questions

Fbuf setup negotiates addresses suitable for all participating AddressSpaces before mapping, rather than attempting to reconcile conflicting placements after pointers escape. For F at X in A and Y in B, pointer X is not usable by B merely because B maps F at Y; the no-intra-AddressSpace-alias rule prevents simply adding X while retaining Y. Reservation/conflict handling and the exact machine-local allocation scheme remain D1/D6. Multi-node global/distributed address-space allocation is explicitly out of scope; do not assume a classical SASOS namespace spanning other nodes' RAM or allocated disk space. Global machine-local reservation has raised concerns but is not yet selected or categorically replaced by a particular allocator.

Authoritative revocation must withdraw the relevant access even if clients retain pointers. Subsequent access to a withdrawn, still-unmapped address faults; Thread termination is the expected policy direction, while fault delivery/termination details remain open. **Stale raw pointers after VA reuse:** a revoked virtual address need not remain inaccessible for the lifetime of a surviving Thread. Capability generations are checked on invocation, not on ordinary CPU loads, so once the address is legitimately reused an old raw pointer may reach the replacement mapping without faulting. Outside/higher-level mechanisms prevent such stale application accesses; stronger kernel guarantees are [future work](capabilities-decisions.md#future-work). Frame-capability slot non-reuse is not virtual-address non-reuse. None of this relaxes kernel memory safety, authority and generation checks, completed hardware withdrawal/TLB synchronization, or safe physical-resource reuse, and it does not make an invalid Rust reference sound.

Read-only mapping permissions prevent writes **through that mapping**; they do not make the backing immutable if another AddressSpace has a writable mapping or a device/kernel writer updates it. For ordinary `&[u8]`, the Rust contract additionally requires readable, initialized, properly bounded memory in one valid allocation, remaining valid and unmodified for the borrow. Multiple shared readers are permitted; conflicting mutation or exclusive access is not. These are reference-construction/usage obligations, not conditions that per-mapping permissions establish by themselves. Exact requirements, source references, immutable-sharing transitions, and fbuf protocols are discussed in [capabilities design](capabilities-design.md).

Bootstrap is explicit: Kickstart is authorized to establish the first Untypeds covering available memory and initial KeyTables for predefined Threads, then hand authority onward to the components/Threads running the system. This is bootstrap authority, not permission for arbitrary runtime callers to manufacture Untypeds. Boot allocations and reserved/live regions must remain accounted for and unavailable for conflicting allocation; exact initial Thread/AddressSpace lists, table capacities/slots, and incarnation-bearing handoff records remain to be specified. Well-known slots are conventions for locating granted capabilities, never a way to manufacture them. Debug-console authority is an explicit bootstrap/delegation choice, not an entitlement implied by knowing its slot.

**Selected handoff:** the nucleus is inert at boot and performs no initialization. The nucleus object model is exposed as a lib so Kickstart (the one-time boot code) constructs the initial `Nucleus` in memory carved from a boot Untyped's unused watermark range, installs the boot Thread, its AddressSpace, and initial grants (boot Untyped at `KeySlot::BOOT_UNTYPED`, debug console at `KeySlot::DEBUG_CONSOLE`), and records the carved address via the exported `nucleus_set_anchor` setter. The nucleus reads that anchor (an `AtomicUsize`) under a separate kernel lock on every syscall. There is no lazy `NUCLEUS` fixture, `BOOT_DOMAIN_STORAGE`, or in-nucleus bootstrap ceremony. This selects the handoff mechanism and the inert-nucleus principle; the exact predefined-Thread list, capacities, and incarnation-bearing handoff records remain open.

## Target responsibilities

| Component | Responsibilities | Exclusions |
|---|---|---|
| Shared ABI definitions, initially within `libs/object` | Object/operation IDs, wire rights and errors, slot and identifier formats, fixed-layout records, shared constants, checked conversions | Kernel pointers, allocation, scheduler policy, syscall execution |
| Userspace interface in `libs/object` | Typed local handles, request encoding, result decoding, slot-management conveniences, sound ownership/mapping abstractions | Treating phantom types as authority; assuming success or object permanence |
| Transport in `libs/syscall` | Architecture register/SVC mechanics and declared inputs, outputs, and clobbers | Per-object policy, duplicated error interpretation, undocumented IPC layouts |
| Nucleus entry and scheduler integration | Validate raw register widths, establish invocation context, save/complete blocked calls, encode results, arrange handoff after borrows/guards end | Scheduling while arbitrary object references or incompatible locks remain live |
| `kernel/nucleus/src/api` | Decode operations, resolve caller-relative capabilities, authorize all participants, coordinate transactions, encode typed results | Persistent capability storage, userspace handle types, duplicated object state machines |
| `kernel/nucleus/src/objects` | Capability entries/tables, resource identity and lifetime, pools, domain/DCB management, queues and typed state transitions | Raw syscall register decoding or userspace scheduler policy |
| Architecture backend | Frame/layout validation, page tables, ASIDs, mappings, TLB/device synchronization | Generic syscall decoding or generic delegation policy |

`KeyEntry` is kernel capability storage even though it currently lives under `api/`. `ObjectType` is shared ABI, not kernel-private state. `NucleusObject` maps kernel object kinds; it is not a universal raw `invoke(op, args)` interface. Architecture associated types retain static selection of implementations; generic invocation decodes once before reaching hardware-specific operations.

The ABI portion must be testable without booting a kernel. The ABI is tied to its architecture, so its tests run natively on a host of that architecture (today AArch64, through `just test-object-host`), alongside the transport wrappers; they need not be separable from the syscall assembly (maintainer decision, 2026-10-08). A separate ABI crate is optional; the responsibility split matters more than an immediate directory or crate reshuffle.

## Vocabulary and identity

- **`KeySlot`**: an index into a particular table's capacity. The shared baseline is a `u32`; wire arguments still arrive in wider registers and require checked conversion. Within a `RawKey` the low 32 bits are a table-relative address: the table's guard packed above the slot index (see the approved key package below). A bare `KeySlot` argument (for example a destination slot) is an index only — its guard-position bits are reserved zero.
- **`Key<T>` / typed key wrapper**: names the particular capability incarnation obtained by the caller, not a slot's future occupant. Altered or invalidated authority must fail invocation with an explicit inconsistency error. The approved encoding and diagnostic schema are specified below. Constructing or copying a Rust wrapper does not mint kernel authority or perform capability Copy. Normal authorized resource operations need not change capability incarnation; Frame remapping is explicitly such an operation, with detailed mapping semantics below.
- **`KeyEntry`**: a fixed-size kernel capability value containing object kind, rights, badge/other authority metadata, and an object handle or inline region description. It is not a userspace record.
- **seL4 terms:** a CSpace is a `KeyTable`, a VSpace is an `AddressSpace`, and a TCB is a `Thread`.
- **keytable**: the capability table ordinary invocations resolve keys in and the table that carries its well-known slots. *Keytable* is Vesper's own name for it; this term is used consistently in Vesper documentation and code. The selected PPC model associates exactly one keytable with each AddressSpace; all Threads executing in that AddressSpace share it. The current model resolves the current Thread's live AddressSpace and its immutable table binding; there is no per-Thread table field.
- **Kernel object/resource**: persistent state with a separately managed lifetime. Multiple entries may refer to it when its contract permits.
- **Thread identity**: kernel-validated identity/incarnation for a schedulable execution entity. Shared scheduler records key this identity; reuse must not let stale records identify a new Thread.
- **Owned slot, mapping, or invocation context**: a stronger wrapper only where the runtime and kernel can enforce its ownership contract. Do not infer ownership from a plain `Key<T>`.

Cross-domain operations identify the destination through authority over the destination table/domain, not by reinterpreting a sender-local slot in the receiver's table. A destination slot is explicit or reserved through a documented protocol.

### Selected local-key and storage foundation

- **Caller-local keys:** ordinary invocation uses the current Thread's KeyTable implicitly. A key contains a slot and capability incarnation, not an explicit table/thread identifier or a path through a guarded table hierarchy. Sending its numeric value to another Thread does not transfer authority; authorized derivation/installation creates a recipient-local capability/key. Explicitly authorized table-management operands select other tables without changing ordinary invocation locality.
- **Key fields:** use a 32-bit table-relative address and a 32-bit capability incarnation initially, without imposing a permanent total-key-size constraint. The table-relative address packs the table's guard (top `32 − size_bits` bits) above the slot index (bottom `size_bits` bits): a smaller table donates its unused slot bits to the guard, never to incarnation. If an embedded type tag is selected later, take its 8 bits from the guard field, narrowing the guard and leaving the index and incarnation widths unchanged. The initial format has no embedded type tag; any future type inclusion requires an explicit format change and cannot replace authoritative checks. The initial wire format is specified below; future format evolution remains D9.
- **Table capacity:** a KeyTable's Retype `size_bits` selects its capacity — `2^size_bits` entries, `size_bits` from 1 to 20. The table object records its own capacity (its header is authoritative for internal bounds), and the carve size is derived from `size_bits` — header plus per-entry storage and incarnation counters — not from a fixed struct size. A Thread's implicit table must hold the well-known bootstrap slots, so its capacity is at least 4 entries (`size_bits ≥ 2`); the boot table keeps 256 entries (`size_bits` 8). Validate decoded indices against the table's capacity and keep allocation/accounting, iteration bounds, bootstrap consumers, and layout tests synchronized. Runtime resizing is not part of this scope; table replacement/rebinding must not silently resurrect old keys. A live Thread cannot rebind its implicit invocation table to a new logical table with restarted counters; physical relocation must preserve the same logical identity and counters.
- **Independent identities:** retain distinct slot-incarnation and shared-object allocation/lifetime checks. Never silently wrap generations and resurrect old identities; the initial capability incarnation is 32 bits. Kernel allocation references use pool identity, index and a 32-bit allocation generation (64-bit generations are [future work](capabilities-decisions.md#future-work)); authoritative metadata survives payload retirement, identity state must not restart on backing reuse, and generation exhaustion prohibits further reuse of that allocation identity. Thread/private-state integration and concrete metadata ownership remain implementation work. A committed Move invalidates the source key and yields a destination-local key while preserving appropriate per-capability state. Ordinary resource-state changes do not automatically replace capability identity. The initial lifecycle uses derivation rather than in-place authority changes, and exhausts individual slots instead of wrapping, as specified below.
- **Storage provenance and privacy:** all managed object storage and its metadata must be backed/accounted by Untypeds, with Kickstart establishing the boot resources described below. Retyping memory into a KeyTable makes its backing kernel-private, including against direct access by the caller: the caller receives a capability to manipulate the table, not a mapping of its entries. **Selected allocation invariant:** Retype operates only on an Untyped and allocates from its unused watermark (waterline) range, which has no outstanding access. It does not convert an arbitrary live Frame/object or perform access withdrawal as part of ordinary allocation. Enforce provenance, non-overlap, watermark accounting and kernel-private backing; any future reclamation must finish withdrawing access before making memory available to Retype again. This selects allocation semantics, not approval of the current sketch's failure handling or placeholder pools.
- **Serialized kernel foundation:** enforce single-core execution for now; SMP is deferred until the basics are sound. Use an owning kernel access context with short-lived guarded references, and release guards before scheduling. Masking local interrupts is not by itself an enforcement mechanism against another core or unsafe reentry. Even under one lock, identical source/destination tables or shared objects must be resolved without constructing overlapping mutable references. Pending work retains checked identities/reservations, not Rust references. **Concrete guarded-access selection:** entries referencing concrete pooled objects store only a checked object identity — pool tag, pool index, and allocation generation — with no raw object pointer in the entry. The `CurrentReturnOnly` Thread selector is not a concrete-object reference and rejects object_id extraction; it still undergoes ordinary checked key lookup. For concrete pooled identities, the access context computes object addresses from pool bases after validating allocation state and generation, so stale pointers cannot be dereferenced. Authoritative allocation/generation/retirement metadata lives in per-pool slot records with stable backing independent of object storage; validation always precedes dereference. The owning access context is constructed once per invocation under the kernel lock and is `!Send`; its guards borrow the context, so the borrow checker enforces that guards end before scheduling. Multi-operand operations resolve same-object aliases through explicit pair forms that reject aliased mutable operands up front. Because mutable resolution exclusively borrows the pool for the invocation, the borrow checker prevents holding two mutable guards into one pool; operations needing two objects from the same pool must request them through the pair-resolution API rather than sequential mutable resolution. One pool per type tag is the invariant that makes this sufficient; introducing multiple pools per type requires revisiting cross-pool alias enforcement. Untyped-backed pool ownership and kernel-private KeyTable backing remain Phase 5 implementation work; this selection fixes identity representation, metadata placement, and the access API shape only.
- **Minimal lifecycle scope:** use ordinary KeyTable capabilities with separately grantable source/destination management permissions, without special manager-identity cases. The CopyDerive/Move target-kind allowlist is KeyTable, Frame and, only under its existing `debug_kernel` gate, DebugConsole; Delete accepts an entry of any kind. Preserve failure atomicity and accepted leaks. The selected logical schemas are below; rights bits, wire layout and remaining per-kind enforcement details still need specification. Selective Revoke, mapping teardown, other target kinds and Invocation/Time-specific behavior are not enabled by this decision.

These are contract decisions, not completed implementation tasks. New key/result/bootstrap encodings require a coordinated kernel/userspace migration. Temporary prototype representations are replaceable scaffolding, not compatibility requirements; preserve pre-existing design-intent comments separately from replacing their implementations.

### Approved key identity and wire package

- `RawKey` has a 32-bit table-relative address and a 32-bit incarnation. Encode explicitly as `(u64::from(incarnation) << 32) | u64::from(address)`; decode all 64 input bits, without relying on Rust struct layout or transmutation. The address packs the table guard in bits 31:`size_bits` and the slot index in bits `size_bits−1`:0, where `size_bits` is the resolving table's capacity exponent. `Key<T>` adds only typing ergonomics: it is a copyable, non-owning handle, with no capability derivation on Copy or deletion on Drop. Keys are opaque to userspace: `RawKey::slot()` returns the full low 32 bits including the guard, and a bare index is recovered only by the kernel against a table's `size_bits`.
- **Table guards:** each KeyTable has one guard, chosen by the caller at Retype and fixed for the table's lifetime — changing it would invalidate every minted key, so no later guard mutation exists. The guard is stored in the KeyTable capability payload — `{address, guard, size_bits}` — which is kernel-visible only; CopyDerive/Move copy it verbatim, so every capability naming a table carries its guard. Every key minted by installation into a table carries that table's guard, including keys to KeyTable capabilities themselves, and a presented key whose guard bits do not match the resolving table is rejected before indexing. The caller's own-table guard is sourced from its self-table capability at the well-known `SELF_KEYTABLE` slot, which must name the caller's recorded table; an invocation whose `SELF_KEYTABLE` entry is missing, is not a KeyTable capability, or names a different table is rejected with a defined error before any key validation (implemented provisionally as `InconsistentKey`/`CapabilityInvalidated` on the invoked key; the exact status is D9). Deleting the self-table capability therefore disables invocation entirely — a self-inflicted, defined outcome. Guards are userspace-chosen namespace values, not kernel secrets: anyone holding a valid key into a table can read its guard from the key's address bits, so the guard's contribution is binding keys to their table — cross-table key confusion fails the guard check instead of colliding on slot and incarnation — rather than hiding entries from a table's own users.
- Ordinary `CapInvoke` uses the complete packed caller-local key in `x0`, full-width checked operation decoding from `x1`, and six arguments in `x2..x7`. Outputs remain status in `x0` and two result/detail words in `x1..x2`. This initial 64-bit key is not a permanent key-size limit. There is no slot-only compatibility fallback; all participating components must be rebuilt together.
- Each slot retains its last-issued incarnation independently of entry occupancy. Never-used slots have counter zero. Installation commits counter + 1 and the entry together; the first issued incarnation is 1. Failed installation changes neither. Delete clears the entry without changing its counter. At `u32::MAX`, a live entry remains usable and deletable, but no further installation into that slot is possible. Move commits a fresh destination-local key and removes the source atomically.
- Validation order for a selected table is zero incarnation, guard mismatch, slot bounds, never-issued slot, slot-incarnation mismatch, invalidated/deleted entry, then authoritative object lifetime before dereference. The lifetime check follows the operation's kind and rights checks: a capability whose pooled object is no longer live (freed, retired, or replaced by a later allocation) fails with `InconsistentKey`/`ObjectRetired` on the key that named it, including an identity copied out of an Invocation entry (the target AddressSpace at `Invocation.Call`). Identities the kernel holds itself (a Frame's recorded mapping, a PageTable's installed parent, the current Thread's AddressSpace) carry no submitted key and keep their operation-specific errors. Caller/table authority must be established before inspecting entries in another table. `InvalidKey` describes invalid key inputs; the shared `InconsistentKey` describes changed capability/object identity. Neither returns a replacement key. Rights failures remain distinct. Delete may clean up a matching entry naming a retired object without dereferencing the retired payload.
- New statuses: `InvalidKey = 26`, `InconsistentKey = 27`, `KeySlotExhausted = 28`. For key errors, `x1` is the submitted packed key; `x2` contains reason in bits 0–7 and the offending input-register index in bits 8–15 (0–7), with all higher bits zero. Invalid-key reasons: ZeroIncarnation = 1, SlotOutOfRange = 2, NeverIssued = 3, GuardMismatch = 4 (the submitted key's guard bits do not address the resolving table). Inconsistency reasons: SlotIncarnationMismatch = 1, CapabilityInvalidated = 2, ObjectRetired = 3. For KeySlotExhausted, `x1` is the destination `u32` slot and `x2` is zero. Existing slot-oriented errors remain available for vacancy/destination operands; superseded status numbers are never recycled. Unknown reasons, operand indices and extension bits remain losslessly observable through the shared decoder.
- CopyDerive `0`: `x0` invoked source-table key (caller-local), `x2` source key selector (source-table-local), `x3` destination-table key (caller-local), `x4` vacant destination slot (a bare index: its guard-position bits are reserved zero), `x5` requested rights, `x6..x7` reserved zero. Success returns the issued destination-local packed key in `x1` — carrying the destination table's guard — and zero in `x2`. The key is directly invocable only when that destination table is the caller's implicit table. Logical Move/Delete contracts remain as selected; their full operation schemas and the table-rights matrix must be finalized before activation.
- Bootstrap must hand recipients the actual keys produced by installation; knowing a slot convention supplies neither an incarnation, nor the table guard, nor install authority. Slot-only constructors and the fake slot-zero initialization SVC are replaceable prototype mechanisms. The bootstrap handoff and general bootstrap layout are specified by the bootstrap contract; Untyped-backed storage remains a separate contract area. This package introduces no runtime bootstrap syscall.

## Object type numbering

The wire object type is one byte. **Bit 7 distinguishes core from architecture-specific types**; bits 6–0 are the kind index within that category.

- Core: `wire_type = core_index`, range `0x00..=0x7f`.
- Architecture: `wire_type = 0x80 | arch_index`, range `0x80..=0xff`.
- An architecture index such as `Frame = 0` is not the complete wire value `Frame = 0x80`.
- Unsupported known kinds and unknown/reserved kinds fail explicitly. A reserved ID does not advertise implementation support.

### Canonical core IDs

These follow `CoreType` in `libs/object/src/object_type.rs` and are the migration target for all constants, conversions, dispatch, error details, tests, and documentation.

| Core kind | Decimal | Wire hex |
|---|---:|---:|
| Null | 0 | `0x00` |
| Untyped | 1 | `0x01` |
| KeyTable | 2 | `0x02` |
| Thread | 3 | `0x03` |
| Time | 4 | `0x04` |
| Scheduler | 5 | `0x05` |
| Brand | 6 | `0x06` |
| Invocation | 7 | `0x07` |
| Notification | 8 | `0x08` |
| EventCount | 9 | `0x09` |
| Reserved | 10–126 | `0x0a..=0x7e` |
| DebugConsole | 127 | `0x7f` |

**No Buffer wire kind:** Buffer is a userspace/libOS construct over frame capabilities — there is no kernel Buffer object, handler, pool, or wire kind. `DebugConsole` keeps its deliberate top-of-core sentinel 127 (D9); core IDs 10–126 remain reserved. Buffer is not a valid Retype kind.

There is no `Domain` wire kind: the execution entity is the core `Thread` and the mapping context is the arch `AddressSpace` (see [Thread and AddressSpace contracts](#thread-and-addressspace-contracts)). Core IDs are grouped by functionality: memory/capability management, execution and scheduling (`Thread`, `Time`, `Scheduler`), then invocation and synchronization (`Brand`, `Invocation`, `Notification`, `EventCount`). Core and architecture kind IDs may be reassigned together to maintain functional grouping. Such changes require a coordinated rebuild; prior IDs are not compatibility constraints.

Each category has one declarative catalogue in `libs/object/src/object_type.rs`. A private declarative macro generates its enum, checked category-index decoder, `ObjectType` aliases, and typed-to-wire conversions. `ObjectType` remains a one-byte wire wrapper that can retain unknown/reserved values; category checks and high-bit encoding remain ordinary Rust code. Catalogue indices are checked at compile time to fit below `0x80`. The macro does not generate handlers, authorization, or object implementations. `Scheduler` and `Brand` are now registered known core kinds, but their operations remain unsupported until their vertical slices are implemented.

The core catalogue follows this table. Its IDs are the agreed functional grouping, and the shared catalogue, dispatch, errors, literal ABI tests, and per-kind documentation must migrate together whenever this mapping changes.

### Architecture ID baseline

| Architecture kind | Index | Wire hex |
|---|---:|---:|
| Frame | 0 | `0x80` |
| PageTable | 1 | `0x81` |
| AddressSpace | 2 | `0x82` |
| ASIDPool | 3 | `0x83` |
| ASIDControl | 4 | `0x84` |
| IOSpace | 5 | `0x85` |
| IOPort | 6 | `0x86` |
| IRQHandler | 7 | `0x87` |
| IRQControl | 8 | `0x88` |
| Reserved | 9–127 | `0x89..=0xff` |

Support depends on the target. IOPort, for example, does not become supported on AArch64 merely because its ID is defined. Core/architecture separation is a dispatch and implementation boundary, not two competing invocation protocols.

## Invocation and wire contracts

### Ordinary control invocation baseline

For the approved AArch64 control-call transport (coordinated migration from the slot-only prototype):

| Direction | Registers | Meaning |
|---|---|---|
| Entry | `SVC` (immediate ignored; wrappers emit `#0`) | Capability invocation |
| Input | `x0` | Packed caller-local key: incarnation in bits 63–32, slot in bits 31–0 |
| Input | `x1` | Operation number |
| Input | `x2..x7` | Six operation arguments |
| Output | `x0` | Status; zero means success |
| Output on success | `x1`, `x2` | Two result words |
| Output on failure | `x1`, `x2` | Error-specific details |

**Register preservation** (maintainer decision, 2026-10-08): an ordinary invocation writes only `x0..x2`. Every other general-purpose register (`x3..x30`, including the argument registers `x3..x7`), SP and the NZCV flags are returned exactly as the caller left them, and execution continues after the `svc`. This holds for success and rejection alike, and for an invocation that blocks and is resumed later. PPC `Invocation.Call`/`Thread.Return` are not ordinary invocations and follow their own register contract below. Exercised by `libkicktest::registers` in `capability-test` (immediate success and rejection) and `sync-test` (blocking resume).

The exception entry validates the exception class and permitted origin before capability dispatch. **The SVC immediate is ignored** (maintainer decision, 2026-10-08): there is exactly one syscall, capability invocation, and there will not be more, so every `svc #n` is the same invocation. Wrappers emit `#0`. Giving the immediate any meaning requires a contract revision first. Other faults follow their own exception path; a user-copy fault must not recursively become a capability invocation.

Extended IPC outputs are [future work](capabilities-decisions.md#future-work).

Every operation's shared contract must specify: ID, argument widths and units, caller-relative slot interpretation, required authority, result shape, blocking behavior, ownership changes, and failure/partial-completion behavior. Pointer arguments specify virtual versus physical address, length, direction of access, and record layout. Ordinary buffers are caller virtual memory, not unchecked physical addresses. Validate access against the caller's protection/authority context, not just whether the kernel can dereference an address. Define input stability across validation/use and buffer lifetime across blocking; copying, pinning, or revalidation are implementation choices, but mutable user records cannot change the authorized request unnoticed (D1/D6/D7).

Decode full-width inputs before narrowing. Reject unknown opcodes, unrepresentable slots/lengths, invalid alignment, and unknown rights/flag bits. A requested-rights word with any bit outside `Rights::all()` fails with `InvalidOperation` before authority checks (shared `Rights::from_wire`). Check addition, shifts, rounding, and multiplication. Spare arguments are not authority and must not acquire undocumented meaning; any reserved-zero requirements belong to the operation schema.

**Strict wire-argument convention:** unless an operation explicitly specifies otherwise, argument words it does not use are transmitted as zero — every ordinary `protected_call*` transport variant zeroes its unused argument registers — and the kernel rejects a nonzero unused word with a defined error instead of ignoring it. `Thread.Return` ignores `x4..x7`. Rationale: a fully determined request encoding is easier to validate and formalise, keeps per-operation wire tests exact, and lets a future optional argument attach meaning to a word that old callers already transmit as zero. The alternative — the kernel ignoring unused words — was considered and deferred; adopting it later is an ABI change to be recorded here first.

### Errors and evolution

`libs/object/src/syscall_status.rs` defines the shared statuses; `CapError::code()` (kernel and client) and `decode_syscall_result` (client) use the same constants, and literal ABI tests pin every value. Object-type details carry the full wire `ObjectType`, except the `Unsupported*Type` statuses, which carry the category-local index.

| Status | Name | `x1` | `x2` |
|---:|---|---|---|
| 0 | `SUCCESS` | result word | result word |
| 1–10 | `UNKNOWN`, `NULL_CAPABILITY`, `INVALID_DOMAIN`, `INVALID_POINTER`, `INSUFFICIENT_RIGHTS`, `NOT_MAPPED`, `ALREADY_MAPPED`, `INVALID_OPERATION`, `ASID_POOL_EXHAUSTED`, `NO_ASID_ASSIGNED` | 0 | 0 |
| 11–13 | `INVALID_SLOT`, `EMPTY_SLOT`, `SLOT_OCCUPIED` | slot | 0 |
| 14, 17, 20 | `NOT_CORE_TYPE`, `NOT_ARCH_TYPE`, `INVALID_OBJECT_TYPE` | wire type | 0 |
| 15, 18 | `UNKNOWN_CORE_TYPE`, `UNKNOWN_ARCH_TYPE` | raw byte | 0 |
| 16, 19 | `UNSUPPORTED_CORE_TYPE`, `UNSUPPORTED_ARCH_TYPE` | local index | 0 |
| 21 | `TYPE_MISMATCH` | expected wire type | found wire type |
| 22, 23 | `INSUFFICIENT_MEMORY`, `POOL_EXHAUSTED` | 0 | 0 |
| 24, 25 | `INVALID_SIZE`, `INVALID_FRAME_SIZE` | size | 0 |
| 26, 27 | `INVALID_KEY`, `INCONSISTENT_KEY` | submitted key | reason \| operand index << 8 |
| 28 | `KEY_SLOT_EXHAUSTED` | slot | 0 |
| 29 | `MISSING_INTERMEDIATE` | virtual address whose walk stopped | 0 |
| 30 | `PHYSICAL_ALIAS` | physical base of the conflicting mapping | 0 |
| 31 | `COUNTER_OVERFLOW` | 0 | 0 |
| 32 | `INVALID_STACK` | offending value | reason ([PPC](#admission-order-and-invalidstack)) |
| 33 | `NESTING_DEPTH` | saved-continuation count | 0 |
| 34 | `UNEXPECTED_RETURN` | local `x1` | local `x2` (client-synthesized) |

Known errors require exactly these details, with unused words zero. Anything else — an unknown status, an unrepresentable detail or reason, a nonzero unused word — decodes as `CapError::UnknownResponse`, which keeps the nonzero status and both words verbatim; it is a client representation, not a wire status. There are no per-family wire error spaces: typed client errors wrap the shared decoded result. A new error meaning (cancellation, partial completion and so on) needs an explicit shared definition first (D9).

Kernel and userspace changes to IDs, layouts, and meaning are coordinated migrations. **Current-consumer compatibility policy** (maintainer decision, 2026-10-08, D9): the kernel and every component are built from one tree and rebuilt together. There is no ABI version discovery and no compatibility guarantee between builds; superseded numbering gets no compatibility encoding. A caller learns that an operation or kind is unsupported only from its error (`InvalidOperation`, `UnsupportedCoreType`, `UnsupportedArchType`). Version/support discovery for separately built components is an open question, decided before such components are supported. No performance claim or `repr` annotation substitutes for a layout/round-trip test.

### Object operation baseline

The following is the operation-ID vocabulary per family. It records numbering and contract qualifications, not implementation status: per-kind status is tracked in the [object-types catalogue](object_types/README.md). Entries labeled deferred or unresolved require contract decisions before activation; do not silently reuse their numbers.

| Family | Existing operation IDs / intended vocabulary | Contract qualification |
|---|---|---|
| Null | None | Never a usable capability |
| Untyped | Retype `0` | Explicit destination; batch wire schema and all-or-nothing semantics selected, see [allocation](#allocation-and-representation) |
| Thread | Return `0`, Grant `1`, Suspend `2`, Resume `3`, Retire `4` | `Return` is selected only for the explicit `CurrentReturnOnly` selector, never `Named(ObjectId)`; no Thread-management rights or concrete Thread target. Return is dispatched; underflow and retired-source faults are delivered to the fault handler (see [fault delivery](#fault-delivery)). `Retire` selected — teardown of a non-current named Thread under `RETIRE` authority; see [thread contracts](#thread-and-addressspace-contracts); Grant overlaps KeyTable delegation; control/time authority must be explicit; Suspend/Resume remain deferred with D7/D8. Activation is `AddressSpace.Activate` (see [architecture families](#object-operation-baseline)) |
| Scheduler | ShareRegion (ID TBD) | Selected direction: share one already scheduler-mapped Frame per call; exact operation ID, register schema, rights, declared-table format, and failure details remain open. |
| Brand | None selected | Intended IPI/thread-handler upcall target; operations and binding remain open, implementation deferred until IRQ work. |
| KeyTable | CopyDerive `0`, Move `1`, Delete `2`, Revoke `4` | `3` unassigned; lifecycle semantics below, not raw entry copying |
| Time | Donate `0`, Split `1`, Merge `2`, Query `3` | Budget conservation and donation lifecycle; D8 |
| Invocation | Call `0` | Protected Procedure Call: the source Thread migrates into the exported procedure's target protection context and returns on procedure completion. Under the common `CapInvoke` ABI, `x0` is the Invocation capability, `x1` is `Call` (`0`), and `x2..x7` are six `u64` Call inputs. The target entry uses two leading dummy arguments in `x0` and `x1`; the six real `u64` arguments remain unchanged in `x2..x7`, with no register shuffle. The dummy arguments are ignored by the target and are not stack metadata. Invocation is Call-only with a mandatory `NonZero<u64>` function address, not an optional entry or Return form. Return uses `Thread.Return` `0` on `CurrentReturnOnly`. Results and selected stack/context directions are specified below; remaining authority, transfer and lifecycle decisions stay open. Construction authority is defined under `AddressSpace.CreateInvocation`. |
| Notification | Signal `0`, Wait `1`, Poll `2` | Coalescing bitmap; badge and waiter delivery decisions remain |
| EventCount | Advance `0`, Await `1`, Read `2` | Monotonic progress and threshold wait; overflow policy and wire schema selected: `Advance` `x2` delta (`SEND`), `Await` `x2` target + `x3` timeout (`RECV`), `Read` no arguments (`RECV`), two-word results, broadcast wakeups — an advance completes every queued `Await` whose target it satisfies |
| DebugConsole | Write `0` | Debug-only prototype behind explicit `debug_kernel`; checked user-memory access and explicit authority remain deferred |
| AddressSpace | Activate `0`, Retire `1`, CreateInvocation `3` | Activate/Retire and CALL-only Invocation capability construction are active; `CreateInvocation` uses the schema and authority below. ID `2` is unassigned. |
| Architecture families | Frame Map `0`, Unmap `1`, GetAddress `2`; PageTable Map `0`, Unmap `1`; ASIDControl, I/O, IRQ control | Frame and PageTable schemas selected (see [mapping](#mapping-and-sharing)); Frame operation ID `3` is unassigned; ASIDControl/I/O/IRQ remain deferred with their draft handlers inactive |



### DebugConsole debug-only exception

DebugConsole is **not a generally available capability**. Its handler, bootstrap grant, userspace wrapper, and boot demonstration require the opt-in Cargo feature `debug_kernel`, disabled by default. This feature identifies a debug kernel independently of Cargo's optimization profile: the embedded build recipes use `--release` even for debugging. Neither `qemu` nor `jtag` implicitly enables it. Build nucleus and kickstart together, for example with `just build rpi3 qemu,debug_kernel`; production kernels must omit `debug_kernel`.

Keep the current pointer-based Write mechanism for trusted debugging only; do not add an inline-byte operation or change the wire schema in this slice. The canonical type ID `127` and operation ID `0` remain reserved/defined regardless of feature availability. Without the kernel feature, no console capability is installed and DebugConsole dispatch is unsupported (an absent slot still returns the normal lookup error).

Temporary debug-only leeway is not a safety or isolation guarantee. The existing EL1 bootstrap caller, broad grant, unchecked physical/direct-map pointer interpretation and copying, C-string limitations, and missing user-copy/rights enforcement remain migration gaps. Client errors now propagate through the shared decoder. The domain-zero fallback has been removed: the boot fixture explicitly selects its first domain after creation, while absent runtime caller identity fails lookup. The stateless console handler now validates the capability's type through a shared entry borrow without deriving an object reference from its payload; this removes the console-specific mutable casts, not the general storage/lifetime gaps. Actual console output currently requires `qemu`; enabling `debug_kernel` alone does not add a hardware output backend. Deferred improvements and alternatives are recorded beside `kernel/nucleus/src/api/debug_console.rs::invoke`. General availability requires the normal supported-operation criteria, including explicit caller/rights, bounded caller-authorized memory access with fault recovery, defined byte/length/partial-output semantics, checked exception/argument decoding, and observable results. D1/D3/D4/D6/D9 remain open beyond this limited availability decision.

## Authority, slots, and capability lifecycle

### Authorization

Rights express permissions for a specific object kind. The existing compact `u8` representation is a baseline, not a completed rights model. Reusing bit positions across kinds is permissible only with unambiguous per-kind meaning. In particular, a read-only mapping must not require a bit that also grants write access to that same resource.

Required semantic checks include:

| Operation family | Authority that must be established |
|---|---|
| Retype | Allocate from source untyped; create requested kind; install into destination table |
| Copy/derive/move | Access source entry, delegate/transfer that kind, mutate destination table; requested rights are a subset |
| Delete/revoke | Manage the target entry or derivation scope; no implicit authority over unrelated descendants/resources |
| Thread control | Named target authority for grant/suspend/resume/retire; budget authority where scheduling consumes time. `Thread.Return` is separate: only `CurrentReturnOnly` permits the invoking Thread's own current continuation pop, with no Thread-management rights |
| AddressSpace control | Control the target address space (activate/retire the translation context) |
| Frame mapping | Access backing, authorize target translation/protection context, restrict requested permissions/attributes |
| Invocation | Authorize entry into the exported procedure and any explicitly selected argument/resource transfer |
| Notification/EventCount | Signal/advance versus observe/wait; apply authorized badge/bit policy |
| Time | Own/delegate budget and authorize its target; no creation of budget from memory alone |
| IRQ/I/O | Control the specific hardware resource and notification/binding destination |
| DebugConsole | Possess explicit console-use authority |

The exact bit assignments, badge width, badge-zero semantics, and operation-to-rights matrix must be finalized in D4. Do not continue the current `u16`/`u32`/`u64` badge disagreement or narrow badges silently.

### Slot conventions

The selected well-known layout is Null `0`, `KeySlot::THREAD_RETURN` `1`, self AddressSpace `2`, parent Thread `3`, self KeyTable `4` (`KeySlot::SELF_KEYTABLE`), boot Untyped `5`, boot ASID pool `14`, and debug console `15` (`KeySlot::DEBUG_CONSOLE`); the boot test's boot-table Retype destinations remain `6–13`. This Return representation change keeps Slot(1) and every other slot unchanged. `THREAD_RETURN` locates a granted `CurrentReturnOnly` Thread entry, not a magic raw key: use the actual target-table-local packed key, including guard and incarnation. AddressSpace provisioning installs it before activation. Ownership of well-known constants and the rest of the general bootstrap handoff remain D4 decisions; research slot sketches do not override this layout.

Table capacity is selected at Retype (`size_bits` 1–20; the boot table keeps 256 slots). Do not assume that every slot fits a 64-bit pending-notification bitmap; define a bounded notification index/registration or a larger representation. Empty slots, reserved slots, and valid entries need explicit table invariants. Inserting null cannot increase occupied count, and arbitrary entry mutation cannot bypass table bookkeeping.

### Copy, move, deletion, and revocation

- **Copy/derive** installs another permitted authority, with no amplification and with per-kind derived state. Rust handle copying is not this operation.
- **Move** changes the slot holding an authority and preserves its appropriate per-capability state. The source is invalidated only when the destination installation commits.
- **Delete** removes an entry. Whether it also retires a resource depends on other capabilities, in-flight use, mappings, and the object's contract.
- **Revoke** retires a defined descendant/scope of authority. Trusted userspace KeyMaster owns derivation-tree operations; kernel object retirement and userspace subtree revocation/cleanup are distinct operations. Their completion boundaries must not conflate invalid capability invocations with withdrawn hardware access or reusable backing; revoke is not merely clearing a watermark.

Untyped allocation authority must not be duplicated into independent watermarks over the same memory. The existing draft forbids ordinary untyped copying; retain that restriction unless a reviewed shared-allocation authority design replaces it. Frame Copy is a checked capability operation, not manual entry copying and not Map: it produces another permitted capability to the same physical frame without duplicating an active mapping association or installing a PTE. A subsequent Map establishes that cap's mapping in an authorized context, potentially at a different virtual page. Stable frame authority and mutable per-cap mapping association are distinct. Moving a mapped frame preserves the binding needed for teardown. Invocation authority must not be derivable in a way that amplifies its target, rights, or badge authority. Time derivation must conserve budget.

### Selected minimal KeyTable lifecycle

- **Authority changes :** no in-place rights/badge mutation initially. Derive an attenuated capability into another slot and optionally delete the original. CopyDerive preserves badges; badge creation/rebadging is deferred. Ordinary object-state changes remain distinct from capability replacement.
- **Slot exhaustion :** once a slot cannot issue a fresh 32-bit incarnation without reuse/wrap, prohibit further installation into that slot. Existing-capability deletion remains possible, and other slots remain usable. Do not retire the entire table merely because one slot exhausts. The approved key package specifies counter zero before first installation, increment-on-install, retained counters on deletion, and KeySlotExhausted status 28; table replacement must not reset stale-key protection.
- **Inconsistency :** use one shared error with diagnostic reasons distinguishing stale slot incarnation, capability invalidation and underlying object retirement. Do not return a replacement key or silently refresh/retry. The approved key package defines InconsistentKey status 27, its reasons and validation precedence; invalid-key inputs use status 26 and ordinary rights failures retain their own semantics.
- **Checked management operands :** source/target entry selectors include the table-relative address (guard + slot index) and expected incarnation, interpreted in the explicitly authorized table; the selector's guard must match that table. Never silently act on a replacement occupant. Table capabilities themselves are resolved through the caller's implicit table and checked identities/authority; this is not a guarded-table path in an ordinary key. A derived or moved KeyTable capability preserves its payload — address, guard, and `size_bits` — verbatim.
- **Separate table permissions :** distinguish source derivation, source removal/move, destination installation and deletion authority rather than one all-or-nothing Manage permission. **Bit assignments and operation matrix:** KeyTable capabilities interpret the rights field as table-management permissions — bit 0 `DERIVE` (source derivation), bit 1 `REMOVE` (source removal/move-out and deletion), bit 2 `INSTALL` (destination installation), bit 3 reserved for future table administration and not granted initially. CopyDerive requires `DERIVE` on the invoked source-table capability plus `INSTALL` on the destination-table capability; Move requires `DERIVE` + `REMOVE` on the source plus `INSTALL` on the destination (Move is a derive-then-remove, not a bare removal); Delete requires `REMOVE` on the invoked table. Requested rights on CopyDerive must be a subset of the source entry's rights, else `InsufficientRights`; badges are copied verbatim. Per-kind restrictions and rights attenuation still apply.

The following logical schemas retain operation IDs and require atomic, ownership-preserving failure behavior. CopyDerive register packing is selected by the approved key package; **Move/Delete wire schemas:** Move uses `x0` invoked source-table key, `x2` source selector, `x3` destination-table key, `x4` vacant destination slot, `x5..x7` reserved zero, returning the destination-local packed key in `x1` and zero in `x2`. Delete uses `x0` invoked table key, `x2` target selector, `x3..x7` reserved zero, returning zero in `x1`/`x2`. Thread.Grant remains deferred; `grant_to` stays a client-side CopyDerive convenience wrapper.

| Operation | Operands in addition to invoked source/target-table capability | Success |
|---|---|---|
| CopyDerive `0` | Source slot/incarnation, destination-table capability, vacant destination slot, requested rights | Destination-local key; source unchanged; no rights amplification; badge preserved |
| Move `1` | Source slot/incarnation, destination-table capability, vacant destination slot | Destination-local key; source invalidated on commit; rights and per-capability state preserved |
| Delete `2` | Target slot/incarnation | Entry removed; no automatic object retirement |

**Same-slot and cleanup rules:** occupied destinations fail rather than overwrite, including Move to the exact same table/slot; it is not a successful no-op. Different slots within one table are supported, with alias-safe access. Delete may clean up an entry whose object has already retired if the entry's slot incarnation still matches; table-management authority and the table's own lifetime must still be valid. A stale selector must not delete a replacement. Failed operations leave authority/accounting unchanged.

**Allowlist:** CopyDerive and Move accept KeyTable, Frame and debug-gated DebugConsole entries; CopyDerive delegates authority over the same object, never a duplicate. Delete accepts an entry of any kind. Delete removes the entry, not the referenced table/resource. Other per-kind lifecycle operations and general Revoke remain unsupported until their contracts and dependencies are implemented. DebugConsole's existing trusted-debug limitations remain; this does not grant general console availability.

### Lifecycle and authority contract

- `Untyped.Retype` yields creation-origin capabilities carrying permission to control the created object's lifetime. That permission may be delegated in derived capabilities; retirement authorization follows capability permissions, not a separate privileged owner identity. Delegating/donating object authority does not implicitly surrender the creator's control: it retains its authority until it destroys its own capability. This does not resolve the separate consuming CPU-budget semantics of Time.Donate (D8).
- Deleting the last retirement-authorized capability need not retire the object. Correct resource management is the OS's responsibility. A resulting permanently lost retyped allocation is an accepted leak, not a kernel obligation to recover or run an automatic final-capability destructor. Leaked storage remains unavailable for conflicting reuse.
- **Authority split confirmed:** management-capable components receive explicit capabilities/rights to derive, install, and manage capabilities. A Key selects a caller-relative slot/incarnation; KeyTable capabilities authorize table operations. Possession of an ordinary invocable object key does not itself confer table-management authority. Source/destination authorization, rights attenuation, and per-kind invariants remain kernel checks. Possession of the appropriate source/destination KeyTable capabilities and permissions is the general table-management rule; manager-identity exceptions are not part of the contract. The minimal lifecycle's separate permissions and logical schemas are selected above; exact bit assignments and wire schemas still need definition.
- Applications entrusted with direct copying/derivation are also responsible for the associated bookkeeping and become part of the trusted computing base (TCB) of that libOS composition, within their granted authority. Ordinary clients may instead use management services without receiving direct management authority. This is not a privileged execution mode or permission to bypass kernel isolation.
- There may be multiple management components, including hierarchical ones. KeyMaster names a userspace management role, not a mandatory singleton or kernel-special identity. Manager topology, distribution of bookkeeping, and coordination/recovery protocols are libOS composition policy. Do not require universal post-hoc registration with a central KeyMaster or a kernel-managed derivation tree.
- Authorized userspace managers implement derivation policy and subtree revocation/cleanup, potentially in the background after object invalidation. The selected SPeCK-like kernel provides object lifetime checks, checked individual capability/mapping operations, and quiescence/reuse mechanisms. Exact metadata, operation schemas, and synchronization remain implementation work, not grounds to reopen the selected authority split.
- Derived capabilities to one object refer to that same object, not successively nested kernel objects. Invalidating its authoritative lifetime identity/generation must reject all old capability invocations irrespective of table or tree depth. Object retirement triggers KeyMaster subtree cleanup, which need not synchronously erase every dead slot. It does not require a kernel ancestry walk just to detect retirement of that shared object.
- **Retired objects are held until cleanup (maintainer decision, 2026-10-09).** Retiring a pooled object moves its slot to `Retired`, not `Free`: the object is dead, every capability naming it fails with `InconsistentKey`/`ObjectRetired`, and the slot is not reallocated. The slot returns to `Free`, and may be reused with an advanced generation, only after the userspace manager responsible for the object has finished cleaning up the capabilities and derivation metadata that name it. How the manager signals that completion is the open D2 completion handshake. Implementation status: `Thread.Retire` and `AddressSpace.Retire` still free the slot immediately; tracked in the implementation plan.
- An object-wide generation cannot selectively invalidate one branch while retaining other capabilities to the same object. The mechanism and completion contract for selective subtree revocation without object retirement remain open (D2); do not impose kernel ancestry metadata or claim object generations solve this different operation.
- The library OS/authorized resource manager is responsible for correct unmap-before-invalidation orchestration. Trust follows granted management authority, not every Thread's use of a libOS. Invalidating capability authority before withdrawing installed access is a resource-management error in that entrusted layer, not a requirement for a recipient to cooperate after invalidation. Kernel mapping primitives supply hardware transitions; premature-reuse checks, retirement prerequisites, and the manager/kernel completion handshake remain D2/D6. Malicious recipients must not defeat completed access revocation or gain access to unrelated reallocated backing. System-wide safe reuse still requires withdrawal of stale mappings and in-flight access.

See [capabilities design](capabilities-design.md) for the reasoning and [capabilities research](capabilities-research.md) for the seL4/Composite comparison.

Kernel object generations and domain reuse protection are distinct from revocation scopes. Persistent handles must not manufacture shared or exclusive Rust references without an owning/locking access context. Resolve aliases before operations involving two capabilities that may name the same table or object (D3).

## Resource storage and memory contracts

### Allocation and representation

Memory-backed objects are created through authorized untyped retype, including the storage for domains and capability tables. Kernel-private state uses typed pools or equivalently explicit bounded storage. Core and architecture storage remain distinguishable because their layouts and lifecycles differ. Allocating a pool must account for its backing rather than silently supplying a second source of uncharged kernel memory.

Untyped and Frame may store region metadata inline in a `KeyEntry`: physical extent, size/alignment information, memory kind, and appropriate per-capability state. This is an available representation, not a size constraint: introduce shared descriptors, additional identity fields, or different layouts when correctness needs them, then consider optimization later. Inline representation does not imply that shared allocation, mapping, or revocation state can safely be copied per entry.

Separate these quantities:

- physical backing bytes and alignment;
- kernel metadata/storage bytes and alignment;
- capability slots and other bounded bookkeeping;
- authority over non-memory namespaces or budgets.

`size_of::<T>()` is not a general physical layout contract. Zero-sized placeholders do not validate a real retype. Frame sizes are typed, architecture-validated choices; 4 KiB, 2 MiB, and 1 GiB are the present AArch64 4 KiB-granule baseline, not universal promises for every target.

**Retype allocation clarification:** only an Untyped can be the source. Its watermark allocator selects an unused, non-overlapping range with no outstanding CPU/device access; previously allocated objects below the watermark are not eligible for arbitrary retyping. This invariant concerns the candidate allocation, not an assertion that all earlier allocations from the parent Untyped remain unmapped. Ordinary Retype does not scan/unmap live allocations, rewind committed allocation state, or reclaim an arbitrary object. Any future reclamation/reset must separately establish safe availability first; no such reset protocol is approved here.

Retype validates source authority, memory kind/device restrictions, absolute physical alignment, size representability, free capacity, destination authority, and destination vacancy before commitment. Watermark encoding cannot discard sub-alignment allocations. Choose single-object versus batch semantics explicitly; a batch needs a stated all-or-nothing or partial-result contract (D6).

**Retype wire schema:** invoked on the Untyped capability key with `x2` object kind, `x3` `size_bits`, `x4` count, `x5` destination-table key, `x6` first destination slot, `x7` requested rights. Success returns the first destination-local key in `x1` and zero in `x2`; the remaining keys occupy the consecutive destination slots. A batch is all-or-nothing: every destination slot is pre-validated (range, vacancy, remaining incarnation capacity) before any object is initialized, and any failure leaves the Untyped's accounting and the destination table unchanged. A batch is also bounded: at most 256 objects per invocation, rejected with `InvalidOperation` otherwise . Authority is `WRITE` on the invoked Untyped plus `INSTALL` on the destination-table capability. The creatable kinds are `KeyTable` (with `size_bits` selecting the capacity — 1 to 20, `InvalidSize` otherwise — and, packed in the same word `x3`, the userspace-chosen table guard in bits 39:8 above the `size_bits` byte; the guard must fit `32 − size_bits` bits, rejected with `InvalidSize` carrying the requested `size_bits` otherwise, and bits 63:40 are reserved zero; every table in a batch shares the one supplied guard; the carve size is derived from `size_bits` as header plus per-entry storage and incarnation counters; the created capability records `{address, guard, size_bits}`; for every other kind, bits 63:8 of `x3` are reserved zero), `Frame`, `PageTable`, and the pure synchronization kinds `Notification` and `EventCount` (pool-backed kernel metadata like a page table, `size_bits` reserved zero): `Frame` is an architecture kind whose `size_bits` the target architecture validates, rejecting unsupported values with `InvalidFrameSize` (the AArch64 4 KiB-granule baseline is 12/21/30, i.e. 4 KiB/2 MiB/1 GiB); the frame carve is aligned to the frame size, and the capability stores the region inline (`RegionPayload`) like an Untyped. `PageTable` is an architecture kind with a fixed architecture-validated 4 KiB carve (`size_bits` 12 on AArch64, `InvalidSize` otherwise); the carved table is sanitized (zeroed) like a frame — stale descriptors would leak prior contents into hardware walks — and its capability is a checked pool identity over kernel metadata (carve address, installed parent, slot, level), not an inline region. `Untyped` itself is creatable : Retype splits a region into smaller Untypeds — `size_bits` selects the child region size (`2^size_bits` bytes), a region is its own alignment like a frame, the child's watermark starts at zero (fully unused), and the split initializes and sanitizes nothing because it is pure bookkeeping: the bytes become observable only through later carves, each of which already initializes or zeroes. A child smaller than the watermark encoding granularity is rejected with `InvalidSize` (the child's own `size_bits`) — a sub-granular region could not keep its committed carve ends encodable. Every other kind is rejected with `InvalidObjectType`, including kinds whose authority cannot originate from memory alone. A device Untyped is not a valid source for any creatable kind except the `Untyped` split: splitting a device Untyped into smaller device Untypeds touches no bytes — the MMIO hazard does not apply — and the `is_device` flag propagates to the children ; every other kind is rejected with `InvalidObjectType` before any reservation, initialization, or watermark change. Per-kind device capability (for example device frames) and device allocation policy remain D6. Region extents are validated before reservation: a size not representable in the address width, or a base+size beyond the physical address space, is rejected with the region's own `size_bits` as `InvalidSize`. The absolute carve address (base + watermark) is aligned up to the object alignment and the watermark granularity — not just the watermark — and the usable range ends where the watermark encoding does (`u32::MAX << MIN_ALIGN_BITS`).

Before freshly allocated or recycled ordinary RAM becomes observable across a protection boundary, the allocation protocol must guarantee initialization/sanitization so a new owner cannot read a prior owner's data or kernel metadata. Intentional content-preserving delegation/sharing is distinct from fresh allocation. Device memory requires its own policy and must not be blindly zeroed. D6 determines who performs sanitization and how completion is enforced.

**Frame sanitization:** for Retype-carved Frames the kernel performs the sanitization itself: the retype transaction zeroes the carved region's contents before installing the capability and advancing the watermark, so a freshly carved Frame never carries prior-owner or kernel data. Device sources are rejected before reservation, so the zeroing touches ordinary RAM only. Broader duties — other allocation/exposure paths, intentional content-preserving sharing, device-memory policy — remain D6.

Resource creation, split, copy, mapping, and transfer follow the conceptual sequence **validate → reserve → initialize/prepare → commit**. Recoverable failure before commit leaves source accounting and authority unchanged. If hardware or other irreversible work makes that impossible, expose and retain a recoverable partial state rather than losing bookkeeping.

### Mapping and sharing

Mapping identity must contain enough information to locate and retire the real mapping: translation/protection context, virtual address/range, permissions/attributes, and lifetime identity as required by the backend. A compressed virtual address without context is not sufficient for arbitrary VSpace mappings. Avoid unexplained exclusions such as address zero or a 44-bit-only range caused by a storage shortcut.

**Mapping-context identity:** the AddressSpace is the mapping context. A mapping is recorded against the AddressSpace that owns the translation-root backend. Sharing across protection boundaries uses frame capabilities mapped into each participant's own AddressSpace (distinct PTEs); sharing a translation context would merge protection boundaries.

**Mapping foundation:** intermediate page tables are explicitly managed, seL4-style — the kernel never allocates translation structures implicitly, so every byte of a translation context is charged to a Retype carve. A `PageTable` capability is installed explicitly: `PageTable.Map` (op 0; `x2` parent key, `x3` virtual address, `x4..x7` zero) installs the invoked table into a parent named by capability type — an `AddressSpace` capability installs the translation root (the virtual address must be zero, the AddressSpace capability must carry `MAP`, and the AddressSpace's root slot must be vacant), a `PageTable` capability installs one intermediate level (the parent must already be installed, the parent level must be below the leaf level, and the parent slot selected by the virtual address must be vacant). `PageTable.Unmap` (op 1; no arguments) requires the table to be installed and empty (all descriptors zero) and clears the parent reference; unmapping the root clears the AddressSpace's translation-root field. `Frame.Map` (op 0; `x2` target AddressSpace key, `x3` virtual address, `x4` requested rights, `x5` attributes, `x6..x7` zero) walks the target AddressSpace's installed tables and writes the real descriptor: 4 KiB frames install page descriptors at level 3, 2 MiB frames block descriptors at level 2, 1 GiB frames block descriptors at level 1; the virtual address must be frame-aligned and inside the supported virtual-address width. **The explicit target-AddressSpace argument is bootstrap-era mechanism**: it exists so an authorized bootstrap builder can populate an AddressSpace before its Thread can run; once syscall caller identity exists, self-context mapping is the intended ordinary path. The walk requires every intermediate table to be present — a missing level fails with `MissingIntermediate` (status 29, detail 1 the faulting virtual address), not fake success. Mapping permissions are bounded by the frame capability's rights (the requested mask must be a subset). **Execute right:** `EXECUTE` is a real frame right (bit `0x10`; `Rights::all()` covers it). A `Frame.Map` requested with `EXECUTE` — within the permission ceiling — makes the mapping executable at exactly one exception level; without it, every mapping remains UXN|PXN. `EXECUTE` with `WRITE` maps kernel-privilege RW+X executable at EL1 only (AP=00, UXN set: EL0 can neither access nor fetch it — AP=00 alone does not stop an EL0 instruction fetch; EL1 cannot execute EL0-writable pages, and EL0 must not execute writable pages either, preserving W^X). This serves the bootstrap caller, which runs at EL1 inside its AddressSpace's context and needs its image executable there. `EXECUTE` without `WRITE` maps read-only code executable at EL0 only (AP=11, PXN set): EL1 never executes code an EL0 component can map. (2026-10-07, maintainer-directed "close the EL1↔EL0 gap": replaces the interim rule that read-only executable mappings were executable at both levels and that RW+X left UXN clear. There is no read-only EL1-executable mapping; PAN is not enabled because the supported cores — Cortex-A53/A72, ARMv8.0-A — lack FEAT_PAN, which is ARMv8.1.) Execute authority flows only through the frame capability's rights — it is never implicit in the mapping operation. Attributes accept only zero (normal write-back cacheable, MAIR index 0) until the attribute dimension is contracted. A frame capability records its single mapping (owning AddressSpace identity and full virtual address) in its payload — not a compressed virtual address — and `Frame.Unmap` (op 1; no arguments) clears the leaf descriptor through that recorded identity. `Frame.GetAddress` (op 2) requires `GRANT` and returns the physical extent — base and size (the client wrapper names it `get_extent`). Frame operation ID `3` is unassigned and rejected; remapping remains a possible future operation whose authority and effects are open (D4/D6). `CopyDerive` of a Frame produces an unmapped derived capability (capability-only derivation, no active mapping association); `Move` preserves the mapping record; `Delete` of a mapped frame leaves the mapping in place under the accepted-leak model — no automatic unmap or retirement. Carved tables become hardware-live through `AddressSpace.Activate`: unmap's ASID-scoped TLB invalidation is executed on the live context (the boot test observes a withdrawn translation through a same-address remap to different backing). An AddressSpace without a bound ASID has no hardware context to withdraw. Gating the invalidation on live installation may be more efficient in the long term once Thread scheduling exists (maintainer remark).

Mapping permissions are bounded by backing and target-context authority; cache/device attributes are a separate validated dimension. Record a mapping only with the actual hardware transition, and preserve enough state to roll back or finish partial map/unmap failures. Reuse requires completed hardware invalidation, including remote TLB or device translation synchronization where applicable.

Maintainer Frame clarification: Copy and Map are separate. Unmap is mapping-local for origin A as well as derived B. “Origin Unmap” means **capability Revoke on the origin**, not a stronger Frame.Unmap primitive. In the seL4-like distinction, origin Revoke withdraws descendant capabilities/mappings while retaining the origin; removal of the origin's own mapping/capability is separate, and object retirement remains a distinct permission-authorized operation. KeyMaster/libOS orchestrates the selected revocation semantics using kernel mechanisms; this is not approval of a kernel-managed derivation tree.

The same physical Frame may be mapped by different derived caps at different virtual pages in different AddressSpaces, with same-address sharing preferred for fbufs. Mapping that backing at two virtual addresses within one AddressSpace is prohibited, including attempts through different derived caps or overlapping frame extents; the enforcement representation must cover physical overlap, not merely capability identity. Cross-AddressSpace mappings are normally distinct PTEs, not competing ownership of one PTE. Origin-authorized remapping remains a valid operation and does not by itself make the capability inconsistent; initial Map of a copied cap is not an origin-only remap. Whether remap changes only virtual placement or can replace physical backing, how descendant mappings respond, and how to encode/enforce origin-only remap authority remain open (D4/D6). Delegated retirement permission must not be treated as permission for a derived capability to remap.

**Alias-policy enforcement:** `Frame.Map` enforces the prohibition ahead of the hardware transition: the arch layer walks the target AddressSpace's installed tables and rejects a candidate whose physical extent overlaps any live page/block descriptor with `PhysicalAlias` (status 30, detail 1 the conflicting live mapping's physical base), leaving every table and record unchanged. The check is interval-based, so it covers different derived caps and mixed frame sizes by construction, and it walks only the target AddressSpace's root, so cross-AddressSpace aliases remain distinct PTEs and writable sharing is unaffected.

Interim deprovisioning direction: fully revoke the Frame capability and do not reuse that slot for Frames. The precise scope/duration of this no-reuse restriction, exhaustion handling, and interaction with eventual incarnation-safe reuse remain D3/D6; it is not a replacement for object identity or hardware-safe backing reuse.

ASIDs come from an authoritative namespace with binding and safe reuse rules. **Selected:** ASIDs are explicit capability-protected resources (seL4-style): `ASIDPool` and the reserved `ASIDControl` kind name the namespace's resources (an ASID itself is a hardware naming value, not an independently capabilitied object), and an AddressSpace's translation root receives an ASID through an authorized AssignASID-style invocation on an ASIDPool capability. Exact operation schemas, rights bits, binding and safe-reuse rules, and their enforcement remain implementation work.

**ASID binding:** `ASIDPool.Assign` (op 0; `x2` target AddressSpace key, `x3..x7` zero) allocates the lowest free ASID from the invoked pool and binds it to the target AddressSpace's translation root; success returns the ASID in `x1` and zero in `x2`. Authority is `GRANT` on the invoked ASIDPool capability plus `MAP` on the target AddressSpace capability. The target must have a translation root installed (`NotMapped` otherwise) and no ASID yet (`AlreadyMapped` otherwise); pool exhaustion is `ASIDPoolExhausted`. Allocation is the last failing step, so every rejection leaves the pool and the AddressSpace unchanged. ASID pools are boot-provided, not Retype-creatable — ASIDs are a hardware namespace, not memory-backed, so memory authority cannot mint them: Kickstart carves and initializes the boot pool kernel-privately and installs its capability at `KeySlot::BOOT_ASID_POOL`. One pool covers 512 ASIDs of the 16-bit hardware space; ASID 0 is reserved for the kernel's own boot translation context, so the first grant is ASID 1. ASID release is part of `AddressSpace.Retire` (whole-ASID invalidation, then release to the originating pool); hardware-safe ASID reuse and partitioning the 16-bit space across multiple pools remain open (D6). The `ASIDControl` kind stays reserved with no pool or handler: binding goes through `ASIDPool.Assign` directly, and pool creation/partitioning is its eventual scope. IRQ/I/O authority likewise comes from authorized hardware-resource assignment, not arbitrary retype.

**AddressSpace activation:** `AddressSpace.Activate` (op 0; no arguments) installs the invoked AddressSpace's bound translation root into the current hardware translation context (`TTBR0_EL1` with the bound ASID on AArch64), making its tables hardware-live. Authority is `MAP` on the invoked AddressSpace capability (mapping-context authority, consistent with root installation and ASID binding). The AddressSpace must have a translation root installed and an ASID bound (`NotMapped` otherwise — either missing means no hardware context); only the current caller's own AddressSpace may be activated (`InvalidOperation` otherwise); the bounded wait/resume fixture switches selected Threads' contexts separately, and Activate neither changes the caller's logical AddressSpace nor exposes public Thread control. Installation is idempotent for the same context; ASID reuse safety remains open (D6). This is the translation-context installation step of activation only: full Thread Start/Suspend/Resume — initialized execution contexts, execution budget, legal state transitions, and EL0 entry — remains Phase 7 work (D7/D8). A caller that keeps executing under the activated context must have its executable image and execution stack mapped. The trusted fixture maps its retained linked image with `EXECUTE` authority and its source execution stack read/write, execute-never. The API prepares checked metadata; syscall entry installs it after object/Access guards and the kernel lock end, emits the success trace after installation, then returns zeros. This internal outcome adds no wire status or ABI.

Large-data IPC preferentially uses fbufs: setup establishes matching virtual addresses suitable for all participants before mapping shared backing, with explicit synchronization/ownership protocols and unchanged pointers where mappings agree. Ring/buffer pools plus produced/consumed event counts support backpressure without allocation or copying in the hot path. Exclusive, immutable-shared, and mutable-shared modes must be available; multiple writable AddressSpaces are permitted, but their synchronization is an fbuf/libOS protocol obligation. Numeric pointer possession still does not grant mapping authority.

Intended MappedSlice ownership: create/own a private capability and its mapping, expose no further derivations or independently usable management aliases, and unmap on Drop. Cleanup uses the incarnation-bearing capability key and required kernel checks, so a stale guard must be rejected rather than affect a replacement. This is not yet implemented by the current wrapper, which borrows an existing BufferKey. Private mapping ownership does not prevent an authorized ancestor from revoking access and does not imply exclusive access to shared physical contents.

**Revocation takes precedence over client borrows.** Higher-level protocols must coordinate use; stale use after access withdrawal is expected to fault and likely terminate the Thread, subject to the address-reuse caveat above. Do not impose an unrevocable Rust borrow/lease as the default or let an uncooperative recipient veto revocation. Faulting/termination enforces containment, not soundness of an otherwise invalid ordinary Rust reference.

The maintainer favors explicit unsafe caller obligations for shared mapped-memory references because cross-AddressSpace exclusivity cannot generally be guaranteed. Exact API placement and enforceable full-borrow contracts remain D3/D6: ordinary slices require validity plus aliasing/mutation guarantees, and a read-only mapping is not immutable backing when other writers exist. Fbuf access, atomics/protocol operations, or copied snapshots can avoid exposing ordinary references where those guarantees cannot be upheld. Unsafe caller errors must remain confined to granted resources/the offending Thread; they cannot excuse kernel memory unsafety or bypass hardware isolation. **Buffer role:** Buffer is a userspace/libOS construct over frame capabilities — there is no kernel Buffer object, handler, pool, or wire kind; core IDs 5–9 are Scheduler, Brand, Invocation, Notification, and EventCount, and `DebugConsole` keeps 127.

**AArch64 translation-switch prerequisite.** TTBR0 page/block leaves must be non-global (`nG`) so ASID-tagged translations from one AddressSpace cannot satisfy another's accesses; the bootstrap identity map follows this rule too. Invariant TTBR1 kernel mappings may remain global. VA-scoped invalidation encodes `TLBI VAE1IS` as ASID in bits 63:48 and `VA[55:12]` in operand bits 43:0, with descriptor stores ordered before TLBI and completion ordered before reuse/access. Installation of a new table/leaf descriptor also publishes its store before subsequent translation walks/access; unmap's earlier barriers cannot publish a later remap store, and exception return is not a substitute. This is hardware mechanism required by the existing separate-AddressSpace contract, not a new mapping authority or ABI. Supported hardware ASID width versus the boot pool's 512-entry range remains D6 work; small distinct fixture ASIDs do not validate that full namespace.

**Checked translation installation (implemented prerequisite).** `prepare_translation_context` validates AddressSpace incarnation, root/ASID presence, and backend encoding before any scheduling mutation. AArch64 supports the existing L0-rooted, four-level, 48-bit VA / 4 KiB TTBR0 profile (`T0SZ=16`, `EPD0=0`, `A1=0`, no DS/LPA2), aligned roots within both configured and hardware PA widths, and nonzero ASIDs representable by the active hardware/configured width. Stale AS is `InvalidDomain`, missing root/ASID is `NotMapped`, unencodable root is `InvalidPointer`, and unsupported profile/ASID is `InvalidOperation`. Both Activate and wait/resume install copied checked metadata only after guards and the kernel lock end. A rejected park/select preparation preserves Thread contexts, pending records, current selection, and runnable FIFO; it does not undo the wait already admitted by capability dispatch. The trusted fixture treats an impossible scheduling-preparation failure as an invariant failure, not fake wait completion or an invented recovery ABI. Prepared metadata is not a lifetime pin: immediate installation relies on the serialized single-core, masked, non-reentrant trap interval; later/asynchronous use requires fresh validation.

## Thread and AddressSpace contracts

**Bounce execution fixture (selected option A).** Bounce remains part of trusted early EL1 testing, using the existing linked Kicktest image to establish Thread continuation, keytable selection, and real translation-root/ASID switching functionality. Multiple protected EL0 AddressSpaces will be tested when proper component images exist; neither an EL0 image/loader fixture nor a confinement proof is a prerequisite for this functional slice. The per-core kernel-stack contract still applies: EL1t fixture execution uses `SP_EL0` for its own execution stack while traps run on the shared high `SP_EL1`; persistent contexts reside in Thread storage. This is internal trusted fixture/bootstrap mechanism, not a public privileged-Thread creation ABI or a relaxation of the selected hostile-native-code protection model. The two-Thread wait/resume fixture now switches real independently provisioned roots with source ASID 1 and Bounce ASID 2; source activation precedes the first handoff. Content-preserving bootstrap Frame grants cover already-accounted retained image/stack pages, never Retype over live bytes. Both low roots map the complete linked image closure; the source maps its retained low RW/XN execution stack, while Bounce's accounted execution stack and the shared trap stack use the invariant high map. Repeated switches check hardware TTBR0/TTBR1, distinct backing at the same warmed low VA, current-AS own-key success/foreign-guard rejection, execution SP/register survival and wait success/overflow results. This is not same-Thread PPC migration or protected EL0 confinement.

**Structure:** the arch **`AddressSpace`** (index 2) is Vesper's equivalent of seL4's VSpace: it holds the translation root and the bound ASID, and it is the protection/mapping-context boundary. It also has exactly one associated keytable, shared by every Thread executing in that AddressSpace. The core **`Thread`** (index 3) is the execution/scheduling entity: execution context, pending-invocation participation, DCB relationship, and a checked reference to its AddressSpace. Well-known bootstrap slots carry the userspace-visible AddressSpace and keytable capabilities; the kernel resolves the current keytable through the current Thread's AddressSpace.

**Keytable association provisioning.** Bind the keytable as part of AddressSpace construction/provisioning, independently of Thread creation. Bootstrap supplies the initial association; a future runtime AddressSpace-creation path must establish it as part of that provisioning transaction. This does not select a new public creation schema or a separate bind operation. Threads execute with the table of their current AddressSpace, rather than supplying or first-binding a table at Thread creation. The backing is libOS-provisioned and accounted; binding allocates no runtime kernel memory. The association is immutable for the lifetime of the provisioned AddressSpace and its accounted private backing; Thread creation neither supplies nor first-binds a table. Lookup validates the Thread's AddressSpace identity, checks `SELF_KEYTABLE` against the binding and table header, and obtains the guard from SELF. Reclaimable KeyTable identity/backing and complete hostile-EL0 backing protection remain separate open work.

An AddressSpace is a protection-context boundary with kernel-private translation state; a Thread combines kernel-private execution/capability state with a scheduler-shared scheduling record. The kernel-private TCB/execution state is never exposed by mapping scheduler-owned record pages. Creation of either establishes coherent identity across private state, scheduling-record association, keytable ownership, scheduler relationships, and backend protection-context binding. Separate AddressSpaces are independently protected; separate ABI kinds/identifiers are reconciled deliberately, never silently renumbered.

Thread identity lookup checks validity and allocation, not just whether a page exists. Invalid or stale IDs fail without indexing outside storage. Thread teardown retires queued calls, waits, replies, time donations, and other references before reuse. No current thread is an explicit state; it must not silently grant access to thread zero.


**Thread capability selectors:** ordinary management entries use `Named(ObjectId)` and retain checked concrete Thread identity. The Return sentinel instead has an explicit `CurrentReturnOnly` Thread selector: it names neither a function, an AddressSpace, nor a concrete Thread, carries no Thread-management rights, and rejects `object_id` extraction rather than manufacturing a zero/fake identity. Named Thread entries never authorize `Thread.Return`; `CurrentReturnOnly` rejects Grant/Suspend/Resume/Retire. All caller-table, guard, slot-incarnation and entry-presence checks apply before selector/operation handling. Any Thread executing in the same AddressSpace may invoke its AS-shared sentinel, but only that invoking Thread's own current invocation stack may be popped; it grants no authority over another Thread's stack.

Thread control has explicit legal state transitions. First start requires an initialized execution context and valid execution authority. Suspend/Resume must distinguish running, runnable, blocked, faulted, and dying states; resuming a suspended blocked invocation cannot bypass its wait condition or create CPU budget. Specify interaction with pending completion, cancellation, and Time donation before exposing these operations (D3/D7/D8).

### Thread.Retire teardown contract

Thread-control teardown contract:

- **Operation.** `Retire` `4` on the Thread kind: no arguments (`x2..x7` zero, strict zero convention), zeros on success, ordinary two-word result. No new wire status: rejections reuse the existing shared errors (`InvalidOperation`, `InsufficientRights`, `InvalidDomain`, the key diagnostics).
- **Authority.** A dedicated right bit `RETIRE` (`0x20`; `Rights::all()` is `0x3F`), delegable like other capability permissions: retiring a Thread requires `RETIRE` on the invoked Thread capability. This is the Thread-kind instance of permission-based lifetime control — no privileged owner identity.
- **Self-retirement.** The current Thread may not retire itself (`InvalidOperation`): the invocation must return to a surviving caller. Never-returns self-retirement is wanted as soon as feasible — it needs terminal entry-path work (a one-way switch with no return frame) — and is recorded here as intended follow-up, not a silent gap; until it exists, a Thread's final exit is userspace policy (block forever, or have a surviving supervisor retire it).
- **Teardown scope.** Retire cancels every pending record naming the Thread as waiter and purges its queued wakeup (`Nucleus::cancel_thread_pending`), then retires the Thread-pool slot, which stays `Retired` until manager cleanup completes ([retired objects are held until cleanup](#lifecycle-and-authority-contract)). Deliberately out of this selection, recorded as gaps: carved backing reclamation remains deferred per accepted-leak; the keytable association is AddressSpace-scoped, and the fully interrupt-kernel contract has no per-Thread kernel stack; the AddressSpace is untouched by Thread.Retire — address-space teardown is the separate `AddressSpace.Retire` below; a bound ASID stays allocated until `AddressSpace.Retire` releases it; scheduler-shared record retirement/reuse follows the D5 scheduler-record protocol and is not yet implemented. Subsequent invocations of the retired Thread's capabilities fail pool validation with a defined error.

### AddressSpace.Retire contract

The address-space side of teardown:

- **Operation.** `Retire` `1` on the AddressSpace kind (arch dispatch): no arguments (`x2..x7` zero, strict zero convention), zeros on success, ordinary two-word result. No new wire status: rejections reuse the existing shared errors.
- **Authority.** `RETIRE` on the invoked AddressSpace capability — the same delegable lifecycle-control right under its per-kind interpretation.
- **Preconditions.** The current caller's own AddressSpace may not be retired (`InvalidOperation`): the invocation must return to a surviving caller. A translation root must not still be installed (`InvalidOperation`): the root is torn down first through the empty-table-gated `PageTable.Unmap` path, so retirement never tears down live translation structures.
- **Teardown scope.** If an ASID is bound, execute the whole-ASID TLB invalidation, then release the ASID back to its originating pool (closing the ASID-release gap recorded under D6; hardware-safe reuse and multi-pool partitioning remain open). Clear the root/ASID fields and retire the AddressSpace-pool slot, which stays `Retired` until manager cleanup completes ([retired objects are held until cleanup](#lifecycle-and-authority-contract)). Threads still referencing the retired AddressSpace fail generation validation on their next resolution (the stale-identity rule); retiring an AddressSpace with running Threads is a caller-side orchestration error confined by that validation.

Scheduling records live in pages owned by a user-space scheduler and explicitly shared with the kernel through `Scheduler.ShareRegion`. The scheduler must map the Frame into its own AddressSpace before sharing it; the kernel accesses the shared backing through its kernel mapping. There is no publicly visible global DCB view. `Scheduler` marks a Thread authorized to act as a scheduler. Scheduler capability derivation is supported for planned hierarchical schedulers; only the root scheduler creates Threads, then may donate them to subordinate schedulers. Strict tree structure and donation enforcement are required but their operation schemas and mechanism remain to be specified. During boot, scheduling begins only after Kickstart establishes the root scheduler; no pre-scheduler kernel FIFO fallback is required by this contract.

The scheduler may write any bytes in pages it maps, including all scheduling-record fields. Kernel-only TCB/execution state is not in those pages and remains inaccessible to the scheduler. The initial record-field and transition protocol starts from the existing `DcbPage`/TCB division; the exact field-by-field split is still open. `ShareRegion` accepts one Frame per invocation; sharing a frame not mapped into the scheduler AddressSpace is rejected, and a conflicting re-share/replacement is rejected rather than silently changing live records. Thread-to-record lookup uses a scheduler-declared fixed-stride table, with kernel-validated Thread identity/incarnation. Exact table declaration, capacities, slot allocation, publication/snapshot rules, and record reclamation remain open. A record's lifetime does not outlive the Thread incarnation it identifies.

`Brand` is a core capability (wire ID 10) intended as a target for IPI and thread-handler upcalls. Its precise target identity, handler binding, operations, and relationship to `IRQHandler` remain open; design is recorded now and implementation is deferred until IRQ work.

Contracts for the shared ABI:

- Fixed-width fields, explicit layout/alignment, mandatory size/offset assertions, and one shared stride/page-capacity definition.
- Mapping availability and record lifetime are established before safe userspace access; absent pages must not be dereferenced.
- Publication ordering is documented. Release/acquire can publish preceding writes but does not create a coherent snapshot across repeated updates; distinguish independently observed counters from fields requiring a versioned snapshot or equivalent protocol.
- Identity and other non-atomic fields cannot be rewritten concurrently with readers without a sound publication/reuse scheme.
- Units and counter meanings are explicit. Existing DCB time accounting is in nanoseconds.
- Event summaries have a defined relationship to slots, notifications, and consumption; they are not a second uncoordinated event-delivery mechanism.

Visibility is limited to schedulers that receive the shared pages; there is no global export. The selected `ShareRegion` shape and fixed-stride placement direction are recorded above. Exact wire operands/results, scheduler-declared table format and sizing, record initialization, publication/snapshot semantics, event-summary indexing, and safe reuse remain D5 work.

Do not preserve the obsolete 128-byte DCB / 32-per-4-KiB assumptions or current 256-byte DcbPage / 8192-byte DcbPages layout. These are migration findings, not the chosen scheduler-shared record ABI.

## Communication and deferred completion

Communication is two kernel mechanisms: blocking waits on `Notification`/`EventCount`, and the Protected Procedure Call (`Invocation`). Queued rendezvous, reply objects and message passing are userspace compositions over them. Open D7 questions are tracked in [`capabilities-decisions.md`](capabilities-decisions.md).

### Blocking and continuations

The kernel keeps one stack per core and never keeps a Thread's continuation on it. A blocking wait copies the caller's saved user context (registers, SP, ELR, SPSR, origin, `TPIDR_EL0`) into the Thread (`ExecutionContext::Parked`, with the pending-record identity), selects another Thread, rewrites the transient exception frame and unwinds normally to `ERET`. Wake-up restores the parked context with the terminal result in `x0..x2`; teardown drops it. Capability entry admits only execution-stack origins (`CurrentSp0` fixture code or lower `AArch64`) and rejects `CurrentSpx` with `InvalidDomain` before any wait is admitted; kernel-mode traps cannot block. A timer tick preempts the same way: the interrupted context is copied into the Thread (`ExecutionContext::Preempted`), the Thread is requeued behind the runnable FIFO and the front is selected; when nothing is runnable the idle Thread runs (it is never queued). Interrupt-time work that cannot finish on the per-core stack is deferred to return-to-user safe points (not yet implemented).

### Wait, timeout, and cancellation

These rules apply to `Notification.Wait` and `EventCount.Await`:

- **Wait identity.** Pending waits are named by kernel-allocated pending-invocation records with non-wrapping 32-bit generations, never bare slot numbers or Thread indices.
- **Timeout.** One relative `u64`-nanosecond timeout per wait; `u64::MAX` means infinite, zero is invalid. No-wait behavior uses the nonblocking Poll/Read operations. Until the time subsystem exists, finite timeouts are rejected.
- **Cancellation.** Timeout and Thread teardown are the cancellation triggers; there is no third-party wait-cancel operation, and a blocked Thread cannot cancel its own wait. `Thread.Retire` cancels the torn-down Thread's pending waits (`Nucleus::cancel_thread_pending`) and purges its queued wakeup. Timeout-driven cancellation and wait-specific error encodings remain open (D8/D9).
- **Terminal transition.** Completion, timeout, cancellation and teardown compete for exactly one terminal transition of a pending record; losers observe its terminal state without mutating it.

### Notification and EventCount

| Primitive | State and meaning | Operations | Typical composition |
|---|---|---|---|
| Notification | Word-sized bitmap; repeated signals to a bit coalesce | Signal ORs authorized bits; Wait blocks until pending and consumes a delivered bitmap; Poll consumes immediately or returns no pending bits | IRQ identity, completion, waking workers to inspect a queue |
| EventCount | Monotonic `u64` progress; advances do not coalesce | Advance adds a checked delta and returns progress; Await waits for `value >= target`; Read observes progress | Producer/consumer backpressure, streaming, per-reader progress tracking |

Signal/Advance/Poll/Read do not block but can fail validation/authorization. A `Signal` wakes at most one waiter, which consumes the delivered bitmap; broadcast-style observation uses independent `EventCount` readers, whose reads and awaits never consume the counter. Condition checks, registration, signal/advance, cancellation and wakeup form one race-free protocol.

**EventCount overflow (D7/D9).** `Advance` requires a nonzero delta. An advance whose sum would exceed `u64::MAX` fails with `CounterOverflow` (status 31, zero details) and leaves the counter unchanged; every queued `Await` completes with the same error, and a woken waiter may re-`Await`. The counter never wraps or saturates.

**Notification authority.** A Notification is Retype-creatable from ordinary memory (`size_bits` reserved zero); its capability is a checked pool identity. `Signal` requires `SEND`, `Wait`/`Poll` require `RECV`. The signaled bits come from the capability's badge when it is nonzero, otherwise from the caller-supplied argument word. Retype installs badge zero, so the argument path is the one reachable today; badge derivation is D4.

**Payload publication (D7).** A `Signal`/`Advance` acts as a release on its caller's behalf; observing the value or bitmap it produced — through a wakeup, an already-satisfied wait, `Poll`, `Await` or `Read` — acts as an acquire. A producer's payload writes before the operation are visible to the consumer's reads after observing it, including buffer-slot reuse. On the current single-core kernel, trap entry/exit and the kernel lock provide this; multicore needs real Acquire/Release kernel atomics. DMA and device writes carry their own coherency and completion obligations.

### Invocation: Protected Procedure Call

An `Invocation` capability identifies an entry point in a target AddressSpace. `Invocation.Call` migrates the calling Thread into that AddressSpace; `Thread.Return` migrates it back. No server Thread exists, and there is no reply capability: Return on the current-relative return key is the only return path. Each AddressSpace has exactly one KeyTable, shared by its Threads; Call switches translation context and KeyTable to the target's, and Return restores the source's (resolved from the source AddressSpace saved in the continuation). Nesting is Composite-style: a bounded per-Thread stack of continuations.

**Provisional conventions.** Two conventions work end-to-end but are not frozen ABI: the `x9` Call-time target-SP transport, and the experimental native body return (an `extern "C"` body returning a `#[repr(C)]` struct of two `u64` in `x0`/`x1`). Freezing each is a maintainer decision ([`capabilities-decisions.md`](capabilities-decisions.md)); nothing else gates it.

#### Construction: `AddressSpace.CreateInvocation` (op 3)

| Register | Meaning |
|---|---|
| `x0` | Key of the target AddressSpace (requires `GRANT`) |
| `x2` | Entry function address, nonzero |
| `x3` | Destination KeyTable key (requires `INSTALL`) |
| `x4` | Vacant destination slot (bare index) |
| `x5`, `x6` | Stack extent `[base, end)` |
| `x7` | Minimum downward headroom `M`, a direct `u64` byte count |

Any holder of those two rights may export; the loader is not an exclusive exporter and there is no interface registry. The installed capability carries only `CALL` authority, the target AddressSpace identity, the `NonZero<u64>` entry address (stored as supplied, with no mapping or executable check) and the validated stack extent and `M`. Success returns the destination-local key in `x1` and zero in `x2`; every failure preserves state and authority. A zero address fails with `InvalidPointer`.

The extent must be nonempty, inside the target's user VA range (the exclusive end may equal the range's upper bound), and `base`, `end` and `M` must each be multiples of 16; no page alignment is required. `M` must be positive and fit the extent (`M <= end - base`; equality admits only `SP = end`). The target's export setup chooses the extent for its own stack-pool and concurrency policy; protecting against stack overflow (guard pages) is the component's concern.

#### Call: `Invocation.Call` (op 0)

Inputs: `x0` the Invocation key, `x1 = 0`, `x2..x7` six `u64` arguments, `x9` the target entry SP (provisional). Operation `1` is unknown (`InvalidOperation`); opcode zero means Call because operation meaning follows the looked-up kind.

The SP must be 16-byte aligned with `base < SP <= end` and `SP - base >= M`. This is numeric range validation only: the kernel does not walk page tables or prove the extent mapped and writable, and it does not switch or provide the target's stack. Pushing past the depth limit fails with `NestingDepth` (status 33, `x1` = current saved-continuation count, `x2` = 0, so `(33, 16, 0)` when full) before anything changes.

On success the kernel pushes a continuation, migrates the Thread, installs the target translation context after all guards and the kernel lock end, and enters the target with:

| State | Target entry (successful Call) | Source resumption (successful Return) |
|---|---|---|
| `x0`, `x1` | Zero — two dummy arguments | `SUCCESS (0)` / first payload word `r0` |
| `x2` | First argument, unchanged | Second payload word `r1` |
| `x3..x7` | Remaining arguments, unchanged | Zero |
| `x8..x18` | Zero (including the consumed `x9`) | Zero |
| `x19..x30` | Zero; the source values are saved privately | Exact saved source values |
| NZCV | Zero | From the saved source SPSR |
| Other SPSR controls | Inherited from the saved source SPSR | Exact saved source SPSR |
| `TPIDR_EL0` | Zero | Saved source value |
| SP / PC | Submitted SP / entry address | Saved source SP / PC after the Call `svc` |

The target keeps the source's execution mode and interrupt masks: Call changes AddressSpace and KeyTable, never privilege. A rejected Call or Return changes no registers beyond the ordinary `x0..x2` result.

#### Admission order and `InvalidStack`

Stop at the first failure. Every stage before the commit preserves state and authority.

- **CreateInvocation:** (1) operation, capabilities, types, rights and live target identity; (2) zero address → `InvalidPointer`; (3) extent and headroom checks; (4) destination slot bounds and vacancy; (5) install.
- **Call:** (1) operation, Invocation key and `CALL` right, live target identity; (2) SP checks; (3) target translation context readiness; (4) depth; (5) push and commit. Hardware installation happens after guards and locks end.

Stack failures report `InvalidStack` (status 32): `x1` is the submitted offending value (never a computed difference) and `x2` the reason. Reason 0 is invalid.

| ID | Reason | Condition | `x1` |
|---:|---|---|---|
| 1 | `ExtentEmpty` | `end == base` | `end` |
| 2 | `ExtentInverted` | `end < base` | `end` |
| 3 | `BaseOutsideUserRange` | Base outside the target user VA range | `base` |
| 4 | `EndOutsideUserRange` | End beyond the permitted user extent | `end` |
| 5 | `BaseMisaligned` | Base not 16-byte aligned | `base` |
| 6 | `EndMisaligned` | End not 16-byte aligned | `end` |
| 7 | `MinimumHeadroomZero` | `M == 0` | `M` |
| 8 | `MinimumHeadroomMisaligned` | `M` not 16-byte aligned | `M` |
| 9 | `MinimumHeadroomTooLarge` | Ordered extent, `M > end - base` | `M` |
| 10 | `SpMisaligned` | SP not 16-byte aligned | `SP` |
| 11 | `SpOutOfRange` | `SP <= base` or `SP > end` | `SP` |
| 12 | `SpInsufficientHeadroom` | In range, `SP - base < M` | `SP` |

Within a stage, CreateInvocation checks reasons 1–2, 3–4, 5–6, 7, 8, then 9 (computing `end - base` only once ordering holds); Call checks 10, 11, then 12 (computing `SP - base` only once in range). For example `base == end == 0x1001` reports `ExtentEmpty`, and an SP that is both misaligned and out of range reports `SpMisaligned`.

#### Invocation stack

Each Thread's pool entry holds a fixed array of 16 continuation records (the depth is a shared constant; no runtime allocation, no per-Thread kernel stack). A record holds the source AddressSpace identity, source return PC and SP, source SPSR and exception origin, source `x19..x30`, source `TPIDR_EL0`, and an 8-byte Call timestamp: 152 B per record, 2432 B per Thread. No KeyTable field is needed (one table per AddressSpace) and no target field (that is the Thread's current AddressSpace while migrated). Nested Calls save each immediate source's own snapshot; restoration never trusts target code or its stack.

On Return, the time since the top record's stamp is charged to the source Thread's own consumed time. Broader attribution across scheduler hierarchies is deferred, and the arithmetic stays inert until a real clock exists (D8).

#### Return: `Thread.Return` (op 0 on `CurrentReturnOnly`)

Inputs: `x0` the caller-table-local packed key of `KeySlot::THREAD_RETURN` (Slot 1), `x1 = 0`, payload `r0`/`r1` in `x2`/`x3`. `x4..x7` are ignored: they need no initialization and carry no meaning. Return pops only the invoking Thread's own top continuation and resumes the source as in the table above.

**The return key.** The Slot 1 entry is a Thread capability with the explicit `CurrentReturnOnly` selector, never `Named(ObjectId)`: it names no Thread, function or AddressSpace, carries no Thread-management rights, and has no object identity to extract. It is not per-invocation authority — the per-Thread invocation stack is the validation state (Composite's cap-0 sentinel, NOVA's implicit reply slot) — so every Thread in an AddressSpace may use the same entry. Only the kernel constructs it: `KeyTable::bind_address_space`, the only issuer of the binding an AddressSpace is created from, installs it into a vacant never-issued Slot 1 at incarnation `KeySlot::THREAD_RETURN_INCARNATION` (1), accepts an existing sentinel unchanged on rebinding, and rejects a foreign entry (`SlotOccupied`) or a vacant previously issued slot (`InvalidOperation`). The key is therefore deterministic (`ThreadReturnKey::provisioned`). The builder passes it to the component's init, which records it for the export adapter (`export::init_return_key`); the channel is part of the open init-handoff design. Lookup is ordinary — guard, incarnation, bounds, presence, no bypass — so a component that deletes its own sentinel gets an ordinary lookup error.

**Return classification.**

| Condition | Outcome |
|---|---|
| Depth zero (nothing to return to) | Fault, kind 1 (`IllegalReturn`), delivered at the Return `svc`; nothing popped |
| Saved source AddressSpace no longer live | Fault, kind 2 (`ReturnTargetRetired`); nothing popped |
| Return on a named Thread entry; Grant/Suspend/Resume/Retire on `CurrentReturnOnly` | `InvalidOperation` |
| Empty, stale or wrong-guard key | Ordinary lookup error |

These two faults have no recoverable path: without a valid continuation the Thread cannot continue after Return. Thread teardown releases the records. A live saved source whose translation context is not ready currently fails with the ordinary preparation error and no pop (classification open, see [`capabilities-decisions.md`](capabilities-decisions.md)). In trusted EL1t code both faults halt the kernel.

#### Userspace conventions

These are libOS conventions over the kernel ABI, not kernel enforcement; a component may use custom entries and call Return directly.

- **Raw wrappers** (`libsyscall::ppc_call` / `ppc_return`) declare `x0..x18` to the compiler: inputs/results as `inlateout`, everything else as discarded clobbers, with no `nomem`, `readonly`, `pure`, `preserves_flags` or `clobber_abi("C")`. `x18` is ordinary caller-volatile scratch, declared only where the target permits it (Apple hosts running the ABI tests reserve it). The Return SVC is not `noreturn`, since a local rejection returns.
- **Export entry** (`ppc_export!`): `CreateInvocation` receives the address of a per-procedure non-returning wrapper with an `extern "C"` eight-`u64` signature (two dummies, six arguments). Call reaches it by exception return, not `BL`; it calls the linked body with the same positional arguments, then completes the call through `export::complete_export`. It never returns through its entry linkage, retries, or reruns the body.
- **Completion** (`export::complete_export`): spill the body's `(r0, r1)` to a 16-byte, 16-aligned slot on the target stack (export setup budgets it in the published headroom), load the return key, and Return with the payload in `x2`/`x3`.
- **Return helper** (`ThreadReturnKey::return_from_invocation`): returns `Result<Infallible, CapError>`. A successful Return never comes back. An ordinary rejection is `Err` with shared diagnostics and nothing popped. A local `SUCCESS` is reported as `UnexpectedReturn` (status 34, the local `x1`/`x2` verbatim), which promises neither unchanged state nor safe retry.
- **Error handoff:** on `Err`, the adapter calls the image-supplied, link-resolved, non-returning `vesper_thread_return_fault` with five `u64` words: `x0..x2` the error from `CapError::code()`, `x3`/`x4` the original `r0`/`r1`. The handler may diagnose, repair and retry, or terminate; it is userspace reporting, not a kernel fault.

#### Architectural state

- **FP/SIMD:** execution is integer-only. The kernel and components build soft-float with FP/NEON code generation off, and `CPACR_EL1` traps FP/SIMD/SVE at EL0 and EL1; a use is an execution fault. No FP/SIMD continuation state exists.
- **`TPIDR_EL0`:** saved per Thread; zero at target entry, restored on Return, zero for a never-run Thread.
- **`TPIDRRO_EL0`:** zero; nothing writes it.
- **Timer and counter (`CNTKCTL_EL1`):** EL0 may read the virtual counter (`CNTVCT_EL0`, `CNTFRQ_EL0`) only; physical counter and all timers are kernel-owned. `CNTVOFF_EL2 = 0`, so virtual equals physical.
- **Interrupts (`SPSR_EL1.I`):** EL0 Threads run with IRQs unmasked, so the nucleus can preempt them; D, A and F stay masked. The nucleus itself and trusted `EL1t` Threads run masked, except Threads created interruptible (the idle Thread).
- **PMU (`PMUSERENR_EL0`) and debug channel (`MDSCR_EL1.TDCC`):** EL0 access traps; no software breakpoints, watchpoints or step.
- Complete architectural-state isolation beyond this list remains open.

#### Fault delivery

A fault is delivered by the kernel making a forced PPC-style Call on the faulting Thread itself into a fault-handler Invocation, so the handler runs with the faulting Thread's priority and budget and needs no handler Thread. (Park-and-signal to an AddressSpace was rejected: it needs a dedicated waiting Thread.)

- **Handler location.** Each AddressSpace's KeyTable holds its handler at `KeySlot::FAULT_HANDLER` (Slot 16) as an `Invocation`. A fault goes to the handler of the AddressSpace the Thread is executing in — inside a migrated call, the callee. The Invocation may target any AddressSpace (for example a pager).
- **Arguments.** `x2` fault kind (0 CPU exception, 1 Return underflow, 2 Return target retired), `x3` `ESR_EL1`, `x4` `FAR_EL1`, `x5` faulting PC, `x6` faulting SP, `x7` invocation depth. For Return faults `x3`/`x4` are zero.
- **Stack and concurrency.** The handler is entered with SP at its Invocation's extent end. The faulting AddressSpace's handler is busy from delivery until its Return (or until the handling Thread is parked as faulted).
- **Frame and resumption.** Each Thread has one kernel-private fault frame holding the faulting context, so there is one fault level. Delivery pushes an ordinary continuation marked as a fault continuation; the handler's `Thread.Return` first payload word selects the action: `0` retry, `1` skip one instruction, `2` terminate (any other value terminates). A Return fault must use skip or terminate.
- **Unhandled faults.** No handler, a busy handler, a fault inside the handler, a full invocation stack, or a rejected handler stack: the Thread is parked permanently as `Faulted`, the AddressSpace's kernel-private unhandled-fault counter increments, the fault is logged, and the next Thread runs. The kernel does not halt; cleanup is `Thread.Retire`.
- **Scope.** All synchronous non-SVC exceptions from EL0 and the two Return faults. Faults in trusted EL1t code halt the kernel.

### Completion and scheduling boundary

An object transition can complete now, block, or request a handoff. Blocking is not an ordinary successful return followed by an ad hoc scheduler call. Save invocation/continuation state and eventual return values explicitly, and complete the userspace return only when the operation completes or is cancelled.

Scheduling and context switching occur after relevant object references and incompatible lock guards end. A fast direct switch is an optimization of that contract, not a bypass. PPC context and continuation resources must be bounded and accounted. Any time donation associated with a PPC follows the Time accounting contract.

### Aborted-work vocabulary

Maintainer-adopted terminology; these are semantic outcome categories, not new wire statuses or an approved result layout:

| Outcome | Meaning |
|---|---|
| Rejected before admission | This attempt did not start |
| Cancelled before commit | The operation guarantees its defined commit did not occur; this does not erase all possible preparatory effects |
| Completed | The outcome is known, including an operation-specific failure or explicitly reported partial result |
| Outcome unknown | Work may have committed, but the observer cannot establish completion |

Authority validity and operation outcome are separate: revoking authority does not prove that already-delivered work did not commit. A timeout or lost completion alone cannot be reported as cancellation-before-commit. The nucleus provides local invocation, cancellation, and completion mechanisms only. Distributed/remote protocols, retries, deduplication, durable receipts, and network failure policy belong in userspace services, not the kernel. D7/D9 must specify which categories each local operation can produce and their exact ABI representation.

## Time and userspace scheduling

Time is a first-class capability to a bounded CPU budget, not just a timer object. Userspace schedulers implement policy, distribute budget hierarchically, and observe DCBs. The nucleus enforces budget consumption, deadlines, preemption, and authorized transitions.

The nucleus owns the tick: the EL1 non-secure physical timer (`CNTP`), whose interrupt it takes through the privileged interrupt-controller component (see [`irq-controller-placement.md`](irq-controller-placement.md) and [`object_types/arch/irq_handler.md`](object_types/arch/irq_handler.md)). Implementation status: a fixed 10 ms periodic tick preempts round-robin, and `current_time_ns()` reads the physical counter; budgets, deadlines, tickless programming and the operations below are not implemented.

- **Donate** authorizes execution of a target using a defined budget and can suspend the donor until yield, exhaustion, cancellation, or another specified completion.
- **Split** creates a child budget in an explicit destination, reducing the parent's remaining amount only on commit.
- **Merge** combines compatible budgets without double counting; parent/provenance and deadline compatibility are checked.
- **Query** observes remaining budget through the same checked result convention as other operations.
- Expiry/revocation prevents further execution on retired budget and produces an explicit donor/parent continuation outcome. Reclamation accounts for already-consumed time.

Settle whether donation is a temporary loan or permanent transfer, where the remainder resides, and what deletion/yield means before exposing consuming Rust wrappers (D8). Memory-backed storage for Time does not mint positive budget; root budget issuance/replenishment requires explicit scheduler authority and conservation rules.

Time sketches use microseconds; DCB accounting uses nanoseconds. D8 must select wire/internal units, clock and rounding/overflow behavior. Nanosecond internal accounting is a recommendation, not an unannounced ABI change. Multiprocessor ownership and simultaneous donation must not permit spending the same budget twice.

## Decision register

Resolve the decisions needed by a slice before enabling it. An unrelated open decision need not block pure ABI tests or repairs to the active path. Record the chosen contract here, its rationale and compatibility impact, and then update the checklist. Do not infer approval from a suggested default.

| ID | Topic | Selected (contract section) | Still open | Needed before |
|---|---|---|---|---|
| D1 | Protection model | [Protection and system composition](#protection-and-system-composition); [fault delivery](#fault-delivery) | Machine-local namespace/reservation conflicts; per-target protection features; Rust reference contracts for shared memory | Thread/AddressSpace protection and mapping semantics |
| D2 | Management authority and reclamation | [Lifecycle and authority contract](#lifecycle-and-authority-contract) | Checked management-operation schemas; selective subtree invalidation of a live object; completion handshake; safe-reuse enforcement | General derivation/revoke; reclaiming retyped memory |
| D3 | Keys, identity and storage | [Vocabulary and identity](#vocabulary-and-identity); [allocation and representation](#allocation-and-representation) | Thread lifecycle integration; in-place authority mutation; unsafe full-borrow/fbuf protocols; per-kind details | General capability access; Thread teardown; client ownership APIs |
| D4 | Authority, badges, bootstrap | [Authorization](#authorization); [slot conventions](#slot-conventions); [lifecycle and authority contract](#lifecycle-and-authority-contract) | Per-operation rights; badges; Call-only Invocation distribution; bootstrap Threads/AddressSpaces/capacities/grants and handoff records; ownership of well-known slot constants | Exposing those authorities or bootstrap records |
| D5 | Scheduler-shared records | [Thread and AddressSpace contracts](#thread-and-addressspace-contracts) | `Scheduler.ShareRegion` schema and rights; record table declaration and field semantics; Thread creation/donation; event summaries; Brand binding | Scheduler record ABI; Thread lifecycle integration |
| D6 | Memory and ASIDs | [Resource storage and memory contracts](#resource-storage-and-memory-contracts) | Map/remap/revoke/no-reuse schemas; sharing-mode and fbuf protocols; sanitization beyond Retype-carved Frames; ASID reuse and multi-pool partitioning; device rules | Memory reclamation and sharing slices |
| D7 | Communication and PPC | [Communication and deferred completion](#communication-and-deferred-completion) | Invocation distribution and badges; capability transfer; nested/concurrent calls beyond the depth bound; call cancellation and teardown; full architectural-state isolation | Remaining PPC lifecycle work |
| D8 | Time | [Time and userspace scheduling](#time-and-userspace-scheduling) (sketch) | Budget issuance, donation as loan or transfer, unused-budget return, split/merge/expiry, units and clocks, multicore accounting | Time/scheduler vertical slice |
| D9 | ABI evolution | [Invocation and wire contracts](#invocation-and-wire-contracts); [object type numbering](#object-type-numbering) | Status for a missing or mismatched self-table capability; wait-operation errors; version discovery for separately built components | Freezing new schemas; separately deployed consumers |

Detailed open questions are tracked in [`capabilities-decisions.md`](capabilities-decisions.md).

## Definition of a supported operation

A supported operation has one shared schema; a client that preserves results; checked nucleus decoding and authorization for every participating resource; typed state transitions with explicit ownership/failure semantics; documented blocking and teardown behavior; and validation at the appropriate ABI, model, and target-integration levels.

Safety comments describe actual invariants and their owner, not just that a call is unsafe. Layout and round-trip assertions remain enabled. Tests include malformed and adversarial requests, aliasing/identity reuse, capacity failures, cancellation, and rollback where applicable.

Follow the [implementation plan](capabilities-implementation-plan.md) in small dependency-respecting slices. The repository skill at `.agents/skills/capability-refactor/SKILL.md` describes the working procedure; it does not replace the contracts in this document.
