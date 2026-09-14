# Nucleus capabilities: design contracts

## Status and authority

This is the implementation reference for Vesper's capability system across the shared/userspace interface, nucleus syscall API, kernel object state, and architecture backends. It records target contracts, not a claim that the current code implements them.

- **Contract** means an invariant or direction to preserve throughout implementation.
- **Baseline** means a documented existing convention to reconcile across layers, not proof of support or correctness.
- **Open decision** means a choice that must be settled before the dependent feature is implemented. Recommendations are not silently binding decisions.

When implementation and this document disagree, record the discrepancy and migrate the code deliberately. Do not silently change the contract to match a stub. Architectural changes require an explicit decision here and corresponding updates to the [implementation plan](capabilities_implementation_plan.md).

### Sources and supersession

This document consolidates:

- The former `kernel/nucleus/design.md`: object identity and storage, architecture associated types, typed frame sizes, VSpace composition, uniform dispatch, open/closed waits, and separate send/receive timeouts. That file is retired in favor of this reference.
- The research note **Kernel API Surface.md**, located in the Metta notes at `Vesper/API/Kernel API Surface.md`: Composite-inspired delegation and userspace resource managers; Nemesis-inspired shared-address-space direction, DCB observations, event counts, and self-scheduling; seL4-inspired untyped allocation, notifications, and capability operations.
- The cross-layer review of `kernel/nucleus/src/api/`, `kernel/nucleus/src/objects/`, `libs/object/`, and the adjacent syscall transport.

The research is inspiration, not a wire ABI. Its example numbering, object sizes, register layouts, slot allocations, standalone syscall list, Rust ownership claims, and cycle estimates are not authoritative. In particular, **the existing `CoreType` discriminants are the correct core numbering**, not the former conflicting `ObjectType` constants or the earlier review's suggested preservation of those constants.

## Key tenets

1. **Mechanism in the nucleus, policy in userspace.** Resource managers, schedulers, drivers, and applications receive only the authority they need. The nucleus enforces protection, accounting, and safe transitions; it does not choose general allocation or scheduling policy.
2. **Explicit authority, no ambient access.** Possessing a numeric address, domain ID, slot number, or typed Rust wrapper does not confer authority. Kernel-resident capabilities and validated delegation determine what a caller may do.
3. **One capability invocation model.** Object operations, including memory creation and time management, use `CapInvoke`. Ergonomic wrappers are not additional kernel primitives. This does not specify unrelated exception or boot entry points.
4. **Separate handles, capabilities, and resources.** A local slot handle names an entry; the entry carries authority; the resource has its own identity, lifetime, and state. These are not interchangeable.
5. **Allocation and temporal authority are accounted resources.** Memory-backed objects originate from authorized untyped memory and explicitly accounted metadata. Memory allocation alone cannot create CPU budget, a reply to a nonexistent call, an IRQ entitlement, or an available ASID.
6. **Monotonic delegation.** Derivation cannot amplify rights, extent, time budget, or other authority. Copies, moves, and revocation have explicit per-kind semantics.
7. **Revocation precedes safe reuse.** Retirement includes in-flight use, mappings, hardware state, and outstanding operations where relevant. Removing a slot or incrementing a generation is not automatically sufficient.
8. **No fabricated Rust safety.** Type tags and phantom types aid programming but do not prove lifetime, exclusivity, mapping validity, or protection from another domain/device. Safe APIs must establish those guarantees.
9. **Defined failure semantics.** Malformed requests return errors, not panics or truncated valid requests. Pre-commit failure preserves resources and ownership. Partial completion is explicit rather than reported as an ordinary all-or-nothing failure.
10. **Cheap observation, authorized mutation.** Shared read-only DCB observations support userspace scheduling without query syscalls. Mutations remain authorized nucleus operations.
11. **Complementary communication primitives.** Small requests use endpoint/reply IPC; large payloads use shared memory; notifications coalesce event identities; event counts preserve progress. Do not merge these distinct semantics into one primitive.
12. **Bounded and testable work.** Prefer explicit storage limits, typed pools, reserved IPC resources, and no hidden hot-path allocation. Long revocation or teardown work needs a bounded/incremental completion contract.
13. **Complete vertical slices.** An operation is supported only when its shared contract, userspace encoding/decoding, kernel authorization, object transitions, and tests agree. A file or enum variant is not evidence of support.
14. **Correctness before representation optimization.** Add, remove, or expand kernel-object and key fields as necessary for consistent identity, authority, mapping, and lifetime semantics; current sizes, packing, and inline representations are not optimization constraints to preserve. Optimize only in a later measured pass. Every layout change must also update affected allocation/accounting, pool capacities/alignment, strides, DCB page views, shared ABI consumers, and layout assertions/tests. Hardware formats and agreed wire encodings still require deliberate, coordinated treatment.

## Protection and system composition

### Selected D1 architecture (maintainer decisions, 2026-09-05)

- **Threat model:** Domains may execute arbitrary native code, including deliberately malicious assembly, forged pointers, and incorrect unsafe code. Confinement must not depend on safe Rust, cooperative library wrappers, or the confined program obeying protocols. Planned validation includes adversarial agent tasks attempting to circumvent isolation.
- **Explicit trust:** KeyMaster can manipulate keys because it receives the corresponding capabilities, not because its name/identity bypasses authorization. Any application granted equivalent rights is entrusted with those operations within their scope; this is not blanket trust in its inputs or access to unrelated resources. Each Domain remains independent. Derivation-manager policy and the kernel-enforced authority boundary must remain distinct.
- **Protection unit:** Domain and VSpace denote the same protection-context boundary. Distinct Domains must not be treated as threads sharing an unprotected memory context. This semantic identification does not silently equate their current numeric IDs, merge the existing ABI kinds, or mandate identical internal representations; backend binding and lifecycle representation remain D1/D6 implementation work.
- **Single address space:** shared numerical meaning and cheap sharing are the goals, not a mandatory shared translation root or zero context-switch cost. When shared backing is properly mapped at X in both Domains, a pointer X can be passed unchanged. Shared fbufs should preferentially use such common mappings and higher-level synchronization. Fbuf setup must establish virtual addresses suitable for every participating Domain before installing the mappings; shared pointers are published only after setup succeeds. Every shared pointer's pointee needs a corresponding authorized mapping; mapping the pointer-containing fbuf does not grant access to arbitrary addresses stored inside it.
- **Allowed mappings:** no physical frame may appear at two different virtual addresses within one Domain. Across Domains, mapping the same frame at different addresses is allowed; the same address is preferred for fbufs/pointer sharing. Simultaneous writable mappings across Domains are permitted, with fbuf/higher-level protocols managing synchronization. Exclusive, immutable-shared, and mutable-shared resource modes are all supported design requirements; their transition protocols remain D3/D6/D7.
- **Fallback and portability:** use separate protected translation contexts, preserving shared-address conventions where possible. Target range is PowerPC G5 through current Intel, Armv9, and RISC-V machines, potentially higher-end STM32. This is intended portability, not a claim of implemented backends or equivalent MMU/MPU/IOMMU features on every target. A per-target protection/translation feature matrix and handling of unavailable features remain open.

### Protection requirements and boundaries

Other Domains' memory confidentiality/integrity must be preserved except for explicitly granted access; kernel-private memory confidentiality/integrity are always required. Everything except the nucleus runs in userspace by default, with no unconfined promoted services planned initially. Any future promotion would enlarge the trusted computing base and requires an explicit security decision, not an ordinary capability grant.

DMA must be confined through a trusted mediation component or an IOMMU as appropriate to the platform. Software mediation is sufficient only if untrusted code cannot bypass it to program DMA-capable hardware/descriptors; an IOMMU path needs its own mapping and invalidation guarantees. Device-level implementation and unavailable-feature behavior remain D1/D6.

Availability/resource-exhaustion policy belongs to the libOS. The nucleus provides resource isolation/abstraction and IPC: it must preserve charged-resource bounds, checked failure behavior, and isolation even when a Domain bypasses its libOS. Exact resource/time accounting and bounded-work mechanisms remain their respective implementation decisions; this does not move general recovery/allocation policy into the kernel. Timing/cache side-channel resistance is deferred but must remain a design consideration; no side-channel confidentiality guarantee is claimed at this stage.

A shared namespace is not itself an isolation mechanism. MTE/PAC alone are not sufficient confinement for malicious native Domains. Each backend must enforce the selected protection boundary for ordinary loads/stores and privileged operations, independently of capability-invocation checks.

### D1 follow-up decisions and remaining questions

Fbuf setup negotiates addresses suitable for all participating Domains before mapping, rather than attempting to reconcile conflicting placements after pointers escape. For F at X in A and Y in B, pointer X is not usable by B merely because B maps F at Y; the no-intra-Domain-alias rule prevents simply adding X while retaining Y. Reservation/conflict handling and the exact machine-local allocation scheme remain D1/D6. Multi-node global/distributed address-space allocation is explicitly out of scope; do not assume a classical SASOS namespace spanning other nodes' RAM or allocated disk space. Global machine-local reservation has raised concerns but is not yet selected or categorically replaced by a particular allocator.

Authoritative revocation must withdraw the relevant access even if clients retain pointers. Subsequent access to a withdrawn, still-unmapped address faults; Domain termination is the expected policy direction, while fault delivery/termination details remain open. **Maintainer deferral:** a revoked virtual address need not remain inaccessible for the lifetime of a surviving Domain. Preventing stale raw-pointer access after authorized VA reuse is left to outside/higher-level mechanisms for now; kernel temporal-address guarantees/quarantine are far-future work, not a blocker for the current refactor. Capability generations are still checked on invocation but not on ordinary CPU loads, so a raw pointer may access an authorized replacement mapping without faulting. This deferral does not relax present kernel memory safety, authority checks, completed hardware withdrawal/TLB synchronization, or safe physical-resource reuse.

Read-only mapping permissions prevent writes **through that mapping**; they do not make the backing immutable if another Domain has a writable mapping or a device/kernel writer updates it. For ordinary `&[u8]`, the Rust contract additionally requires readable, initialized, properly bounded memory in one valid allocation, remaining valid and unmodified for the borrow. Multiple shared readers are permitted; conflicting mutation or exclusive access is not. These are reference-construction/usage obligations, not conditions that per-mapping permissions establish by themselves. Exact requirements, source references, immutable-sharing transitions, and fbuf protocols are discussed in [Lifetime and authority semantics](lifetime-and-authority.md).

Bootstrap is explicit: Kickstart is authorized to establish the first Untypeds covering available memory and initial KeyTables for predefined Domains, then hand authority onward to the components/Domains running the system (maintainer decision, 2026-09-06). This is bootstrap authority, not permission for arbitrary runtime callers to manufacture Untypeds. Boot allocations and reserved/live regions must remain accounted for and unavailable for conflicting allocation; exact initial Domain lists, table capacities/slots, and incarnation-bearing handoff records remain to be specified. Well-known slots are conventions for locating granted capabilities, never a way to manufacture them. Debug-console authority is an explicit bootstrap/delegation choice, not an entitlement implied by knowing its slot.

**Selected handoff (2026-09-12):** the nucleus is inert at boot and performs no initialization. The nucleus object model is exposed as a lib so Kickstart (the one-time boot code) constructs the initial `Nucleus` in memory carved from a boot Untyped's unused watermark range, installs the boot Domain and initial grants (boot Untyped at `KeySlot::BOOT_UNTYPED`, debug console at `KeySlot::DEBUG_CONSOLE`), and records the carved address via the exported `nucleus_set_anchor` setter. The nucleus reads that anchor (an `AtomicUsize`) under a separate kernel lock on every syscall. The old lazy `NUCLEUS` fixture, `BOOT_DOMAIN_STORAGE`, and the in-nucleus `nucleus_bootstrap_debug_console` ceremony are removed. This selects the handoff mechanism and the inert-nucleus principle; the exact predefined-Domain list, capacities, and incarnation-bearing handoff records remain open.

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

The ABI portion must be testable independently of syscall assembly. A separate ABI crate is optional; the responsibility split matters more than an immediate directory or crate reshuffle.

## Vocabulary and identity

- **`KeySlot`**: an index in a particular domain's KeyTable. The shared baseline is a `u32`; wire arguments still arrive in wider registers and require checked conversion.
- **`Key<T>` / typed key wrapper**: names the particular capability incarnation obtained by the caller, not a slot's future occupant (maintainer decision, 2026-09-05). Altered or invalidated authority must fail invocation with an explicit inconsistency error. The approved encoding and diagnostic schema are specified below; implementation status must be tracked separately from this contract. Constructing or copying a Rust wrapper does not mint kernel authority or perform capability Copy. Normal authorized resource operations need not change capability incarnation; Frame remapping is explicitly such an operation, with detailed mapping semantics below.
- **`KeyEntry`**: a fixed-size kernel capability value containing object kind, rights, badge/other authority metadata, and an object handle or inline region description. It is not a userspace record.
- **Kernel object/resource**: persistent state with a separately managed lifetime. Multiple entries may refer to it when its contract permits.
- **`DomainId`**: identity for domain state and observation, not a capability granting control. Reuse must not let stale references accidentally identify a new domain.
- **Owned slot, mapping, or reply**: a stronger wrapper only where the runtime and kernel can enforce its ownership contract. Do not infer ownership from a plain `Key<T>`.

Cross-domain operations identify the destination through authority over the destination table/domain, not by reinterpreting a sender-local slot in the receiver's table. A destination slot is explicit or reserved through a documented protocol.

### Selected local-key and storage foundation (2026-09-06)

- **Caller-local keys:** ordinary invocation uses the current Domain's KeyTable implicitly. A key contains a slot and capability incarnation, not an explicit table/domain identifier or a path through a guarded table hierarchy. Sending its numeric value to another Domain does not transfer authority; authorized derivation/installation creates a recipient-local capability/key. Explicitly authorized table-management operands select other tables without changing ordinary invocation locality.
- **Key fields (maintainer follow-up):** use a 32-bit slot and 32-bit capability incarnation initially, without imposing a permanent total-key-size constraint. This supersedes the earlier capacity-dependent bit split: a smaller table does not donate unused slot bits to incarnation. If an embedded type tag is selected later, take its 8 bits from the slot field, leaving 24 slot bits and 32 incarnation bits. The initial format has no embedded type tag (maintainer approval, 2026-09-07); any future type inclusion requires an explicit format change and cannot replace authoritative checks. The initial wire format is specified below; future format evolution remains D9.
- **Table capacity:** fixed for now and controlled by an easily changed constant, not inferred from the width of the slot field. Validate decoded slots against the configured capacity and keep allocation/accounting, iteration bounds, bootstrap consumers, and layout tests synchronized with constant changes. Runtime resizing is not part of this initial scope; table replacement/rebinding must not silently resurrect old keys. Retain the current capacity of 256 for this foundation, controlled by a constant rather than field width. A live Domain cannot rebind its implicit invocation table to a new logical table with restarted counters; physical relocation must preserve the same logical identity and counters.
- **Independent identities:** retain distinct slot-incarnation and shared-object allocation/lifetime checks. Never silently wrap generations and resurrect old identities; the initial capability incarnation is 32 bits. Kernel allocation references use pool identity, index and a 64-bit allocation generation; authoritative metadata survives payload retirement, identity state must not restart on backing reuse, and generation exhaustion prohibits further reuse of that allocation identity. Domain/private-state integration and concrete metadata ownership remain implementation work. A committed Move invalidates the source key and yields a destination-local key while preserving appropriate per-capability state. Ordinary resource-state changes do not automatically replace capability identity. The initial lifecycle uses derivation rather than in-place authority changes, and exhausts individual slots instead of wrapping, as specified below.
- **Storage provenance and privacy:** all managed object storage and its metadata must be backed/accounted by Untypeds, with Kickstart establishing the boot resources described below. Retyping memory into a KeyTable makes its backing kernel-private, including against direct access by the caller: the caller receives a capability to manipulate the table, not a mapping of its entries. **Selected allocation invariant:** Retype operates only on an Untyped and allocates from its unused watermark (waterline) range, which has no outstanding access. It does not convert an arbitrary live Frame/object or perform access withdrawal as part of ordinary allocation. Enforce provenance, non-overlap, watermark accounting and kernel-private backing; any future reclamation must finish withdrawing access before making memory available to Retype again. This selects allocation semantics, not approval of the current sketch's failure handling or placeholder pools.
- **Serialized kernel foundation:** enforce single-core execution for now; SMP is deferred until the basics are sound. Use an owning kernel access context with short-lived guarded references, and release guards before scheduling. Masking local interrupts is not by itself an enforcement mechanism against another core or unsafe reentry. Even under one lock, identical source/destination tables or shared objects must be resolved without constructing overlapping mutable references. Pending work retains checked identities/reservations, not Rust references. **Concrete guarded-access selection (2026-09-07):** capabilities store only a checked object identity — pool tag, pool index, and allocation generation — with no raw object pointer in the entry; the access context computes object addresses from pool bases after validating allocation state and generation, so stale pointers cannot be dereferenced. Authoritative allocation/generation/retirement metadata lives in per-pool slot records with stable backing independent of object storage; validation always precedes dereference. The owning access context is constructed once per invocation under the kernel lock and is `!Send`; its guards borrow the context, so the borrow checker enforces that guards end before scheduling. Multi-operand operations resolve same-object aliases through explicit pair forms that reject aliased mutable operands up front. Because mutable resolution exclusively borrows the pool for the invocation, the borrow checker prevents holding two mutable guards into one pool; operations needing two objects from the same pool must request them through the pair-resolution API rather than sequential mutable resolution. One pool per type tag is the invariant that makes this sufficient; introducing multiple pools per type requires revisiting cross-pool alias enforcement. Untyped-backed pool ownership and kernel-private KeyTable backing remain Phase 5 implementation work; this selection fixes identity representation, metadata placement, and the access API shape only.
- **Minimal lifecycle scope:** use ordinary KeyTable capabilities with separately grantable source/destination management permissions, without special manager-identity cases. The initial CopyDerive/Move/Delete target-kind allowlist is KeyTable and, only under its existing `debug_kernel` gate, DebugConsole. Preserve failure atomicity and accepted leaks. The selected logical schemas are below; rights bits, wire layout and remaining per-kind enforcement details still need specification. Selective Revoke, mapping teardown, other target kinds and Reply/Time-specific behavior are not enabled by this decision.

These are contract decisions, not completed implementation tasks. New key/result/bootstrap encodings require a coordinated kernel/userspace migration. Temporary prototype representations are replaceable scaffolding, not compatibility requirements; preserve pre-existing design-intent comments separately from replacing their implementations.

### Approved key identity and wire package (2026-09-07)

- `RawKey` has a 32-bit slot and a 32-bit incarnation. Encode explicitly as `(u64::from(incarnation) << 32) | u64::from(slot)`; decode all 64 input bits, without relying on Rust struct layout or transmutation. `Key<T>` adds only typing ergonomics: it is a copyable, non-owning handle, with no capability derivation on Copy or deletion on Drop.
- Ordinary `CapInvoke` uses the complete packed caller-local key in `x0`, full-width checked operation decoding from `x1`, and six arguments in `x2..x7`. Outputs remain status in `x0` and two result/detail words in `x1..x2`. This initial 64-bit key is not a permanent key-size limit. There is no slot-only compatibility fallback; all participating components must be rebuilt together.
- Each slot retains its last-issued incarnation independently of entry occupancy. Never-used slots have counter zero. Installation commits counter + 1 and the entry together; the first issued incarnation is 1. Failed installation changes neither. Delete clears the entry without changing its counter. At `u32::MAX`, a live entry remains usable and deletable, but no further installation into that slot is possible. Move commits a fresh destination-local key and removes the source atomically.
- Validation order for a selected table is zero incarnation, slot bounds, never-issued slot, slot-incarnation mismatch, invalidated/deleted entry, then authoritative object lifetime before dereference. Caller/table authority must be established before inspecting entries in another table. `InvalidKey` describes invalid key inputs; the shared `InconsistentKey` describes changed capability/object identity. Neither returns a replacement key. Rights failures remain distinct. Delete may clean up a matching entry naming a retired object without dereferencing the retired payload.
- New statuses: `InvalidKey = 26`, `InconsistentKey = 27`, `KeySlotExhausted = 28`. For key errors, `x1` is the submitted packed key; `x2` contains reason in bits 0–7 and the offending input-register index in bits 8–15 (0–7), with all higher bits zero. Invalid-key reasons: ZeroIncarnation = 1, SlotOutOfRange = 2, NeverIssued = 3. Inconsistency reasons: SlotIncarnationMismatch = 1, CapabilityInvalidated = 2, ObjectRetired = 3. For KeySlotExhausted, `x1` is the destination `u32` slot and `x2` is zero. Existing slot-oriented errors remain available for vacancy/destination operands; superseded status numbers are never recycled. Unknown reasons, operand indices and extension bits remain losslessly observable through the shared decoder.
- CopyDerive `0`: `x0` invoked source-table key (caller-local), `x2` source key selector (source-table-local), `x3` destination-table key (caller-local), `x4` vacant destination slot, `x5` requested rights, `x6..x7` reserved zero. Success returns the issued destination-local packed key in `x1` and zero in `x2`. The key is directly invocable only when that destination table is the caller's implicit table. Logical Move/Delete contracts remain as selected; their full operation schemas and the table-rights matrix must be finalized before activation.
- Bootstrap must hand recipients the actual keys produced by installation; knowing a slot convention does not supply an incarnation or install authority. Slot-only constructors and the fake slot-zero initialization SVC are replaceable prototype mechanisms. The private debug boot handoff is described in implementation status below; general bootstrap layout and Untyped-backed storage remain separate work. This package introduces no runtime bootstrap syscall.

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
| Domain | 2 | `0x02` |
| KeyTable | 3 | `0x03` |
| Time | 4 | `0x04` |
| Endpoint | 5 | `0x05` |
| Notification | 6 | `0x06` |
| EventCount | 7 | `0x07` |
| Buffer | 8 | `0x08` |
| Reply | 9 | `0x09` |
| Reserved | 10–126 | `0x0a..=0x7e` |
| DebugConsole | 127 | `0x7f` |

Each category has one declarative catalogue in `libs/object/src/object_type.rs`. A private declarative macro generates its enum, checked category-index decoder, `ObjectType` aliases, and typed-to-wire conversions. `ObjectType` remains a one-byte wire wrapper that can retain unknown/reserved values; category checks and high-bit encoding remain ordinary Rust code. Catalogue indices are checked at compile time to fit below `0x80`. The macro does not generate handlers, authorization, or object implementations.

The core catalogue follows this table; do not renumber `CoreType` to match the old constants. This corrects the previously conflicting IDs for Time, Endpoint, Notification, and EventCount. Kernel and userspace must migrate together; compatibility with the contradictory encoding is not implied.

### Architecture ID baseline

| Architecture kind | Index | Wire hex |
|---|---:|---:|
| Frame | 0 | `0x80` |
| PageTable | 1 | `0x81` |
| VSpace | 2 | `0x82` |
| ASIDPool | 3 | `0x83` |
| ASID | 4 | `0x84` |
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
| Entry | `SVC #0` | Capability invocation |
| Input | `x0` | Packed caller-local key: incarnation in bits 63–32, slot in bits 31–0 |
| Input | `x1` | Operation number |
| Input | `x2..x7` | Six operation arguments |
| Output | `x0` | Status; zero means success |
| Output on success | `x1`, `x2` | Two result words |
| Output on failure | `x1`, `x2` | Error-specific details |

The exception entry validates the exception class, SVC immediate, and permitted origin before capability dispatch. Other faults follow their own exception path; a user-copy fault must not recursively become a capability invocation.

This is **not** an agreed extended IPC message ABI. The endpoint sketches cannot use extra result registers unless the transport and exception return path explicitly support them, including assembly outputs/clobbers (D7).

Every operation's shared contract must specify: ID, argument widths and units, caller-relative slot interpretation, required authority, result shape, blocking behavior, ownership changes, and failure/partial-completion behavior. Pointer arguments specify virtual versus physical address, length, direction of access, and record layout. Ordinary buffers are caller virtual memory, not unchecked physical addresses. Validate access against the caller's protection/authority context, not just whether the kernel can dereference an address. Define input stability across validation/use and buffer lifetime across blocking; copying, pinning, or revalidation are implementation choices, but mutable user records cannot change the authorized request unnoticed (D1/D6/D7).

Decode full-width inputs before narrowing. Reject unknown opcodes, unrepresentable slots/lengths, invalid alignment, and unknown rights/flag bits. Check addition, shifts, rounding, and multiplication. Spare arguments are not authority and must not acquire undocumented meaning; any reserved-zero requirements belong to the operation schema.

### Errors and evolution

`syscall_status` in `libs/object/src/syscall_status.rs` defines success zero, the existing errors 1–25, and the approved key errors 26–28. `CapError::code()` and `decode_syscall_result` use the same error constants; nucleus syscall return handling uses `SUCCESS` and `CapError::code()`. Preserve these status meanings; literal ABI tests independently pin their numeric values. Correct object-type details to the canonical numbering above. Make clear which error detail is a category-local index and which is a full wire ObjectType.

Do not create competing per-family wire error spaces such as the draft `RetypeError`. Typed client errors may wrap the shared decoded result. Unknown future status/detail values must remain observable without panicking or being turned into success. New errors for inconsistency, overflow, cancellation, unsupported behavior, and partial completion require explicit shared definitions, not ad hoc sentinel values (D9). The approved key package assigns statuses 26–28 and defines their diagnostic details. Other future error meanings still require explicit shared contracts.

The shared `decode_syscall_result` decodes the three-word result, preserving both success words. Existing status 1–25 meanings are unchanged; statuses 26–28 use the approved key diagnostics. Known errors require representable details/reasons/operand indices and zero unused bits. Otherwise `CapError::UnknownResponse` preserves the nonzero status and both details verbatim, including future extensions. This fallback is a client representation, not a new wire status. Included Domain/KeyTable/DebugConsole wrappers use it. The key migration changes Rust constructors, source selectors, and CopyDerive/grant result types; downstream callers and exhaustive error matches must migrate. CopyDerive clients return the packed key from the first success word and ignore the second; producers must emit zero there, but no client-side reserved-word rejection was selected.

Kernel and userspace changes to IDs, layouts, and meaning are coordinated migrations. Before separately versioned components are supported, choose ABI compatibility/version discovery and how callers learn which optional operations are available (D9). No performance claim or `repr` annotation substitutes for a layout/round-trip test.

### Object operation baseline

The following preserves existing operation declarations as a starting vocabulary. Except for console write, it is not a claim of end-to-end support. Entries labeled deferred or unresolved require contract decisions before activation; do not silently reuse their numbers.

| Family | Existing operation IDs / intended vocabulary | Contract qualification |
|---|---|---|
| Null | None | Never a usable capability |
| Untyped | Retype `0` | Explicit destination; batch wire schema and all-or-nothing semantics selected (2026-09-13), see [allocation](#allocation-and-representation) |
| Domain | Activate `0`, Grant `1`, Suspend `2`, Resume `3` | Grant overlaps KeyTable delegation; control/time authority must be explicit |
| KeyTable | CopyDerive `0`, Move `1`, Delete `2`, Revoke `4` | `3` unassigned; lifecycle semantics below, not raw entry copying |
| Time | Donate `0`, Split `1`, Merge `2`, Query `3` | Budget conservation and donation lifecycle; D8 |
| Endpoint | Call `0`, Send `1`, Recv `2`; ReplyRecv `3`, Reply `4`, Forward `5` | Prefer explicit Reply authority; `3`/`5` deferred optimizations, `4` conflicting legacy operation to resolve in D7 |
| Notification | Signal `0`, Wait `1`, Poll `2` | Coalescing bitmap; badge and waiter delivery decisions remain |
| EventCount | Advance `0`, Await `1`, Read `2` | Monotonic progress and threshold wait |
| Buffer | Map `0`, Unmap `1`, Query `2` | Dedicated kernel kind versus userspace composition remains D6; retain its type ID |
| Reply | Send `0`, SendWithCap `1`, SendError `2` | One-shot reply authority; exact transfer/cancellation encoding remains D7 |
| DebugConsole | Write `0` | Debug-only prototype behind explicit `debug_kernel`; checked user-memory access and explicit authority remain deferred |
| Architecture families | Frame mapping/query; page-table mapping; VSpace translation/ASID binding; ASID, I/O, IRQ control | Freeze per-operation schemas with the relevant backend; do not treat draft handlers as complete contracts |

Implementation status: `KeyTableOp` provides checked `TryFrom<u64>` and `TryFrom<u32>` decoding of the existing IDs above. The full-width decoder rejects every other value with the existing `InvalidOperation` error, including unassigned `3` and high-bit aliases; the `u32` adapter widens before delegating. The CopyDerive/Move/Delete handler is active with the approved rights matrix and wire schemas. KeyTables are Retype-created carved kernel objects: a KeyTable capability references its carved object through a per-kind payload address rather than a pool identity, and the handler resolves the invoked source and destination table capabilities through the guarded `Access` context from the caller's own table, so `CopyDerive`/`Move` can target a table other than the caller's (cross-table resolution, 2026-09-12; carved-table representation and active runtime Retype, 2026-09-14). The boot Domain's table is carved and initialized kernel-privately by Kickstart. The `Untyped` Retype handler is active with the selected schema, the KeyTable allowlist, and the general device-source rejection. Revoke returns `InvalidOperation`. Exception-origin validation remains unfinished.

### DebugConsole debug-only exception

Maintainer decision (2026-09-05, scoped D4/D9): DebugConsole is **not a generally available capability**. Its handler, bootstrap grant, userspace wrapper, and boot demonstration require the opt-in Cargo feature `debug_kernel`, disabled by default. This feature identifies a debug kernel independently of Cargo's optimization profile: the embedded build recipes use `--release` even for debugging. Neither `qemu` nor `jtag` implicitly enables it. Build nucleus and kickstart together, for example with `just build rpi3 qemu,debug_kernel`; production kernels must omit `debug_kernel`.

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
| Domain control | Control the target domain; budget authority where activation/scheduling consumes time |
| Frame/Buffer mapping | Access backing, authorize target translation/protection context, restrict requested permissions/attributes |
| Endpoint/Reply | Call/send/receive as appropriate; separately authorize any capability transfer and reply delegation |
| Notification/EventCount | Signal/advance versus observe/wait; apply authorized badge/bit policy |
| Time | Own/delegate budget and authorize its target; no creation of budget from memory alone |
| IRQ/I/O | Control the specific hardware resource and notification/binding destination |
| DebugConsole | Possess explicit console-use authority |

The exact bit assignments, badge width, badge-zero semantics, and operation-to-rights matrix must be finalized in D4. Do not continue the current `u16`/`u32`/`u64` badge disagreement or narrow badges silently.

### Slot conventions

The current interface names Null `0`, self domain `1`, parent domain `2`, self KeyTable/manager `3`, and debug console `127`. The research instead put current Time at `3`. **These are conflicting bootstrap sketches, not simultaneous contracts.** D4 must define one bootstrap layout and distinguish a real KeyTable capability from a userspace manager endpoint.

Current tables have 256 slots. Do not assume that every slot fits a 64-bit pending-notification bitmap; define a bounded notification index/registration or a larger representation. Empty slots, reserved slots, and valid entries need explicit table invariants. Inserting null cannot increase occupied count, and arbitrary entry mutation cannot bypass table bookkeeping.

### Copy, move, deletion, and revocation

- **Copy/derive** installs another permitted authority, with no amplification and with per-kind derived state. Rust handle copying is not this operation.
- **Move** changes the slot holding an authority and preserves its appropriate per-capability state. The source is invalidated only when the destination installation commits.
- **Delete** removes an entry. Whether it also retires a resource depends on other capabilities, in-flight use, mappings, and the object's contract.
- **Revoke** retires a defined descendant/scope of authority. Trusted userspace KeyMaster owns derivation-tree operations; kernel object retirement and userspace subtree revocation/cleanup are distinct operations. Their completion boundaries must not conflate invalid capability invocations with withdrawn hardware access or reusable backing; revoke is not merely clearing a watermark.

Untyped allocation authority must not be duplicated into independent watermarks over the same memory. The existing draft forbids ordinary untyped copying; retain that restriction unless a reviewed shared-allocation authority design replaces it. Frame Copy is a checked capability operation, not manual entry copying and not Map: it produces another permitted capability to the same physical frame without duplicating an active mapping association or installing a PTE. A subsequent Map establishes that cap's mapping in an authorized context, potentially at a different virtual page. Stable frame authority and mutable per-cap mapping association are distinct. Moving a mapped frame preserves the binding needed for teardown. Reply authority cannot be copied into independently usable replies. Time derivation must conserve budget.

### Selected minimal KeyTable lifecycle (2026-09-06)

- **Authority changes (3A=A):** no in-place rights/badge mutation initially. Derive an attenuated capability into another slot and optionally delete the original. CopyDerive preserves badges; badge creation/rebadging is deferred. Ordinary object-state changes remain distinct from capability replacement.
- **Slot exhaustion (3B=A):** once a slot cannot issue a fresh 32-bit incarnation without reuse/wrap, prohibit further installation into that slot. Existing-capability deletion remains possible, and other slots remain usable. Do not retire the entire table merely because one slot exhausts. The approved key package specifies counter zero before first installation, increment-on-install, retained counters on deletion, and KeySlotExhausted status 28; table replacement must not reset stale-key protection.
- **Inconsistency (3C=B):** use one shared error with diagnostic reasons distinguishing stale slot incarnation, capability invalidation and underlying object retirement. Do not return a replacement key or silently refresh/retry. The approved key package defines InconsistentKey status 27, its reasons and validation precedence; invalid-key inputs use status 26 and ordinary rights failures retain their own semantics.
- **Checked management operands (4A=A):** source/target entry selectors include slot and expected incarnation, interpreted in the explicitly authorized table. Never silently act on a replacement occupant. Table capabilities themselves are resolved through the caller's implicit table and checked identities/authority; this is not a guarded-table path in an ordinary key.
- **Separate table permissions (4B=B):** distinguish source derivation, source removal/move, destination installation and deletion authority rather than one all-or-nothing Manage permission. **Bit assignments and operation matrix selected (2026-09-07):** KeyTable capabilities interpret the rights field as table-management permissions — bit 0 `DERIVE` (source derivation), bit 1 `REMOVE` (source removal/move-out and deletion), bit 2 `INSTALL` (destination installation), bit 3 reserved for future table administration and not granted initially. CopyDerive requires `DERIVE` on the invoked source-table capability plus `INSTALL` on the destination-table capability; Move requires `DERIVE` + `REMOVE` on the source plus `INSTALL` on the destination (Move is a derive-then-remove, not a bare removal); Delete requires `REMOVE` on the invoked table. Requested rights on CopyDerive must be a subset of the source entry's rights, else `InsufficientRights`; badges are copied verbatim. Per-kind restrictions and rights attenuation still apply.

The following logical schemas retain operation IDs and require atomic, ownership-preserving failure behavior. CopyDerive register packing is selected by the approved key package; **Move/Delete wire schemas selected (2026-09-07):** Move uses `x0` invoked source-table key, `x2` source selector, `x3` destination-table key, `x4` vacant destination slot, `x5..x7` reserved zero, returning the destination-local packed key in `x1` and zero in `x2`. Delete uses `x0` invoked table key, `x2` target selector, `x3..x7` reserved zero, returning zero in `x1`/`x2`. Domain.Grant remains deferred; `grant_to` stays a client-side CopyDerive convenience wrapper.

| Operation | Operands in addition to invoked source/target-table capability | Success |
|---|---|---|
| CopyDerive `0` | Source slot/incarnation, destination-table capability, vacant destination slot, requested rights | Destination-local key; source unchanged; no rights amplification; badge preserved |
| Move `1` | Source slot/incarnation, destination-table capability, vacant destination slot | Destination-local key; source invalidated on commit; rights and per-capability state preserved |
| Delete `2` | Target slot/incarnation | Entry removed; no automatic object retirement |

**Same-slot and cleanup rules (4C=A):** occupied destinations fail rather than overwrite, including Move to the exact same table/slot; it is not a successful no-op. Different slots within one table are supported, with alias-safe access. Delete may clean up an entry whose object has already retired if the entry's slot incarnation still matches; table-management authority and the table's own lifetime must still be valid. A stale selector must not delete a replacement. Failed operations leave authority/accounting unchanged.

**Initial allowlist (4D=A):** the target capability kinds are KeyTable and debug-gated DebugConsole only. CopyDerive delegates permitted authority over the same table or console, not a duplicate table object. Delete removes the entry, not the referenced table/resource. Other per-kind lifecycle operations and general Revoke remain unsupported until their contracts and dependencies are implemented. DebugConsole's existing trusted-debug limitations remain; this does not grant general console availability.

### Maintainer lifecycle and authority decisions (2026-09-05)

- `Untyped.Retype` yields creation-origin capabilities carrying permission to control the created object's lifetime. That permission may be delegated in derived capabilities; retirement authorization follows capability permissions, not a separate privileged owner identity. Delegating/donating object authority does not implicitly surrender the creator's control: it retains its authority until it destroys its own capability. This does not resolve the separate consuming CPU-budget semantics of Time.Donate (D8).
- Deleting the last retirement-authorized capability need not retire the object. Correct resource management is the OS's responsibility. A resulting permanently lost retyped allocation is an accepted leak, not a kernel obligation to recover or run an automatic final-capability destructor. Leaked storage remains unavailable for conflicting reuse.
- **Authority split confirmed (2026-09-06):** management-capable components receive explicit capabilities/rights to derive, install, and manage capabilities. A Key selects a caller-relative slot/incarnation; KeyTable capabilities authorize table operations. Possession of an ordinary invocable object key does not itself confer table-management authority. Source/destination authorization, rights attenuation, and per-kind invariants remain kernel checks. Maintainer clarification (2026-09-06): possession of the appropriate source/destination KeyTable capabilities and permissions is the general table-management rule; do not add manager-identity exceptions. The minimal lifecycle's separate permissions and logical schemas are selected above; exact bit assignments and wire schemas still need definition.
- Applications entrusted with direct copying/derivation are also responsible for the associated bookkeeping and become part of the trusted computing base (TCB) of that libOS composition, within their granted authority. Ordinary clients may instead use management services without receiving direct management authority. This is not a privileged execution mode or permission to bypass kernel isolation.
- There may be multiple management components, including hierarchical ones. KeyMaster names a userspace management role, not a mandatory singleton or kernel-special identity. Manager topology, distribution of bookkeeping, and coordination/recovery protocols are libOS composition policy. Do not require universal post-hoc registration with a central KeyMaster or a kernel-managed derivation tree.
- Authorized userspace managers implement derivation policy and subtree revocation/cleanup, potentially in the background after object invalidation. The selected SPeCK-like kernel provides object lifetime checks, checked individual capability/mapping operations, and quiescence/reuse mechanisms. Exact metadata, operation schemas, and synchronization remain implementation work, not grounds to reopen the selected authority split.
- Derived capabilities to one object refer to that same object, not successively nested kernel objects. Invalidating its authoritative lifetime identity/generation must reject all old capability invocations irrespective of table or tree depth. Object retirement triggers KeyMaster subtree cleanup, which need not synchronously erase every dead slot. It does not require a kernel ancestry walk just to detect retirement of that shared object.
- An object-wide generation cannot selectively invalidate one branch while retaining other capabilities to the same object. The mechanism and completion contract for selective subtree revocation without object retirement remain open (D2); do not impose kernel ancestry metadata or claim object generations solve this different operation.
- The library OS/authorized resource manager is responsible for correct unmap-before-invalidation orchestration. Trust follows granted management authority, not every Domain's use of a libOS. Invalidating capability authority before withdrawing installed access is a resource-management error in that entrusted layer, not a requirement for a recipient to cooperate after invalidation. Kernel mapping primitives supply hardware transitions; premature-reuse checks, retirement prerequisites, and the manager/kernel completion handshake remain D2/D6. Malicious recipients must not defeat completed access revocation or gain access to unrelated reallocated backing. System-wide safe reuse still requires withdrawal of stale mappings and in-flight access.

See [Lifetime and authority semantics](lifetime-and-authority.md) for the decision history, seL4/Composite comparison, and outstanding implementation work.

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

**Retype allocation clarification (2026-09-06):** only an Untyped can be the source. Its watermark allocator selects an unused, non-overlapping range with no outstanding CPU/device access; previously allocated objects below the watermark are not eligible for arbitrary retyping. This invariant concerns the candidate allocation, not an assertion that all earlier allocations from the parent Untyped remain unmapped. Ordinary Retype does not scan/unmap live allocations, rewind committed allocation state, or reclaim an arbitrary object. Any future reclamation/reset must separately establish safe availability first; no such reset protocol is approved here.

Retype validates source authority, memory kind/device restrictions, absolute physical alignment, size representability, free capacity, destination authority, and destination vacancy before commitment. Watermark encoding cannot discard sub-alignment allocations. Choose single-object versus batch semantics explicitly; a batch needs a stated all-or-nothing or partial-result contract (D6).

**Retype wire schema selected (2026-09-13):** invoked on the Untyped capability key with `x2` object kind, `x3` `size_bits`, `x4` count, `x5` destination-table key, `x6` first destination slot, `x7` requested rights. Success returns the first destination-local key in `x1` and zero in `x2`; the remaining keys occupy the consecutive destination slots. A batch is all-or-nothing: every destination slot is pre-validated (range, vacancy, remaining incarnation capacity) before any object is initialized, and any failure leaves the Untyped's accounting and the destination table unchanged. Authority is `WRITE` on the invoked Untyped plus `INSTALL` on the destination-table capability. The initial creatable kind is `KeyTable` with `size_bits` reserved zero; every other kind is rejected with `InvalidObjectType`, including kinds whose authority cannot originate from memory alone. A device Untyped is not a valid source for any creatable kind: Retype from one is rejected with `InvalidObjectType` before any reservation, initialization, or watermark change. Per-kind device capability (for example device frames) and device allocation policy remain D6.

Before freshly allocated or recycled ordinary RAM becomes observable across a protection boundary, the allocation protocol must guarantee initialization/sanitization so a new owner cannot read a prior owner's data or kernel metadata. Intentional content-preserving delegation/sharing is distinct from fresh allocation. Device memory requires its own policy and must not be blindly zeroed. D6 determines who performs sanitization and how completion is enforced.

Resource creation, split, copy, mapping, and transfer follow the conceptual sequence **validate → reserve → initialize/prepare → commit**. Recoverable failure before commit leaves source accounting and authority unchanged. If hardware or other irreversible work makes that impossible, expose and retain a recoverable partial state rather than losing bookkeeping.

### Mapping and sharing

Mapping identity must contain enough information to locate and retire the real mapping: translation/protection context, virtual address/range, permissions/attributes, and lifetime identity as required by the backend. A compressed virtual address without context is not sufficient for arbitrary VSpace mappings. Avoid unexplained exclusions such as address zero or a 44-bit-only range caused by a storage shortcut.

Mapping permissions are bounded by backing and target-context authority; cache/device attributes are a separate validated dimension. Record a mapping only with the actual hardware transition, and preserve enough state to roll back or finish partial map/unmap failures. Reuse requires completed hardware invalidation, including remote TLB or device translation synchronization where applicable.

Maintainer Frame clarification (2026-09-05): Copy and Map are separate. Unmap is mapping-local for origin A as well as derived B. The earlier phrase “origin Unmap” meant **capability Revoke on the origin**, not a stronger Frame.Unmap primitive. In the seL4-like distinction, origin Revoke withdraws descendant capabilities/mappings while retaining the origin; removal of the origin's own mapping/capability is separate, and object retirement remains a distinct permission-authorized operation. KeyMaster/libOS orchestrates the selected revocation semantics using kernel mechanisms; this is not approval of a kernel-managed derivation tree.

The same physical Frame may be mapped by different derived caps at different virtual pages in different Domains, with same-address sharing preferred for fbufs. Mapping that backing at two virtual addresses within one Domain is prohibited, including attempts through different derived caps or overlapping frame extents; the enforcement representation must cover physical overlap, not merely capability identity. Cross-Domain mappings are normally distinct PTEs, not competing ownership of one PTE. Origin-authorized remapping remains a valid operation and does not by itself make the capability inconsistent; initial Map of a copied cap is not an origin-only remap. Whether remap changes only virtual placement or can replace physical backing, how descendant mappings respond, and how to encode/enforce origin-only remap authority remain open (D4/D6). Delegated retirement permission must not be treated as permission for a derived capability to remap.

Interim deprovisioning direction: fully revoke the Frame capability and do not reuse that slot for Frames. The precise scope/duration of this no-reuse restriction, exhaustion handling, and interaction with eventual incarnation-safe reuse remain D3/D6; it is not a replacement for object identity or hardware-safe backing reuse.

ASIDs come from an authoritative namespace with binding and safe reuse rules. Decide whether ASID capabilities are explicit resources or pool-owned VSpace bindings; preserve the registered IDs while this is unresolved. IRQ/I/O authority likewise comes from authorized hardware-resource assignment, not arbitrary retype.

Large-data IPC preferentially uses fbufs: setup establishes matching virtual addresses suitable for all participants before mapping shared backing, with explicit synchronization/ownership protocols and unchanged pointers where mappings agree. Ring/buffer pools plus produced/consumed event counts support backpressure without allocation or copying in the hot path. Exclusive, immutable-shared, and mutable-shared modes must be available; multiple writable Domains are permitted, but their synchronization is an fbuf/libOS protocol obligation. Numeric pointer possession still does not grant mapping authority.

Intended MappedSlice ownership: create/own a private capability and its mapping, expose no further derivations or independently usable management aliases, and unmap on Drop. Cleanup uses the incarnation-bearing capability key and required kernel checks, so a stale guard must be rejected rather than affect a replacement. This is not yet implemented by the current wrapper, which borrows an existing BufferKey. Private mapping ownership does not prevent an authorized ancestor from revoking access and does not imply exclusive access to shared physical contents.

**Revocation takes precedence over client borrows.** Higher-level protocols must coordinate use; stale use after access withdrawal is expected to fault and likely terminate the Domain, subject to the address-reuse caveat above. Do not impose an unrevocable Rust borrow/lease as the default or let an uncooperative recipient veto revocation. Faulting/termination enforces containment, not soundness of an otherwise invalid ordinary Rust reference.

The maintainer favors explicit unsafe caller obligations for shared mapped-memory references because cross-Domain exclusivity cannot generally be guaranteed. Exact API placement and enforceable full-borrow contracts remain D3/D6: ordinary slices require validity plus aliasing/mutation guarantees, and a read-only mapping is not immutable backing when other writers exist. Fbuf access, atomics/protocol operations, or copied snapshots can avoid exposing ordinary references where those guarantees cannot be upheld. Unsafe caller errors must remain confined to granted resources/the offending Domain; they cannot excuse kernel memory unsafety or bypass hardware isolation. Buffer as a kernel object versus a userspace aggregate remains D6; do not remove its public kind merely as cosmetic cleanup.

## Domain and shared DCB contracts

A Domain is a VSpace/protection-context boundary combining kernel-private execution/protection/capability state with a userspace-observable DCB. The kernel-private part is never exposed by mapping DCB pages. Domain creation establishes coherent identity across private state, DCB, keytable ownership, scheduler relationships, and its backend protection-context binding. Separate Domains are independently protected; current Domain/VSpace ABI kinds and IDs require deliberate reconciliation, not silent renumbering.

Domain identity lookup checks validity and allocation, not just whether a page exists. Invalid or stale IDs fail without indexing outside storage. Domain teardown retires queued calls, waits, replies, time donations, and other references before reuse. No current domain is an explicit state; it must not silently grant access to domain zero.

Implementation status: `Nucleus::current_domain_mut` and `current_dcb_mut` now return `None` when no current domain is recorded. Active capability dispatch consequently returns the existing `InvalidDomain` error without looking up a capability in domain zero. The existing boot fixture explicitly selects domain zero after creating the first private-domain pool entry; this preserves its debug path, not a new general bootstrap or execution-authority contract. Private-domain lookup retains pool bounds/allocation checks. Exception-origin/caller binding, incarnation-safe domain reuse, coherent private-domain/DCB allocation, and DCB allocation/publication checks remain unfinished.

Domain control has explicit legal state transitions. First activation requires an initialized execution context and valid execution authority. Suspend/Resume must distinguish running, runnable, blocked, faulted, and dying states; resuming a suspended blocked invocation cannot bypass its wait condition or create CPU budget. Specify interaction with pending completion, cancellation, and Time donation before exposing these operations (D3/D7/D8).

The DCB exposes state, blocking/fault information, time accounting, scheduler relationship, and pending-event summaries sufficient for userspace scheduling. Kernel writes and userspace reads through authorized read-only mappings. Reading a DCB does not grant control of that domain. Maintainer direction (2026-09-05): DcbView is intended to be persistent, not withdrawn like an ordinary revocable buffer mapping. Exact backing/availability guarantees and record reuse/publication remain D5; persistence of the view does not imply persistence of each domain incarnation.

Contracts for the shared ABI:

- Fixed-width fields, explicit layout/alignment, mandatory size/offset assertions, and one shared stride/page-capacity definition.
- Mapping availability and record lifetime are established before safe userspace access; absent pages must not be dereferenced.
- Publication ordering is documented. Release/acquire can publish preceding writes but does not create a coherent snapshot across repeated updates; distinguish independently observed counters from fields requiring a versioned snapshot or equivalent protocol.
- Identity and other non-atomic fields cannot be rewritten concurrently with readers without a sound publication/reuse scheme.
- Units and counter meanings are explicit. Existing DCB time accounting is in nanoseconds.
- Event summaries have a defined relationship to slots, notifications, and consumption; they are not a second uncoordinated event-delivery mechanism.

The research intended system-wide read visibility. That is an information-disclosure choice, not a requirement of zero-syscall observation. D5 must confirm visibility, placement/discovery, sizing, and snapshot/reuse semantics.

Do not preserve the obsolete 128-byte DCB / 32-per-4-KiB assumptions without a layout decision. The review's isolated host-layout check measured the then-current structs as 256-byte DCBs and 8192-byte DcbPages. That is a migration finding, not the chosen future ABI.

## Communication and deferred completion

### Notification and EventCount

| Primitive | State and meaning | Operations | Typical composition |
|---|---|---|---|
| Notification | Word-sized bitmap; repeated signals to a bit coalesce | Signal ORs authorized bits; Wait blocks until pending and consumes a delivered bitmap; Poll consumes immediately or returns no pending bits | IRQ identity, completion, waking workers to inspect a queue |
| EventCount | Monotonic `u64` progress; advances do not coalesce | Advance adds a checked delta and returns progress; Await waits for `value >= target`; Read observes progress | Producer/consumer backpressure, streaming, per-reader progress tracking |

Signal/Advance/Poll/Read do not block, but still can fail validation/authorization. EventCount readers maintain independent positions; reading/awaiting does not consume the counter. Arithmetic must not silently wrap and break monotonicity. Finalize the overflow error policy before activation (D7/D9).

Wait condition checks, registration, signal/advance, cancellation, and wakeup must form a race-free protocol. Multiple notification waiters require an explicit one-consumer versus broadcast contract; do not promise both consuming all bits and delivering those same bits independently to every waiter. Badge-derived signaling versus caller-supplied bits, including zero-badge meaning, is D4. Wakeup summary updates must agree with DCB semantics.

Shared-payload publication is a separate contract from race-free wakeup. D7 must state whether Signal/Advance and an observing Wait/Poll/Await/Read provide release/acquire synchronization or require explicit userspace synchronization. Cover already-satisfied waits, polling, and buffer-slot reuse as well as blocking wakeups. A syscall alone does not establish this guarantee; DMA completion and cache coherency have additional architecture/device obligations.

### Endpoint and Reply

Endpoints are for small-message rendezvous and request/reply. Badges identify authority-bearing sender views, not an unvalidated caller-supplied identity. A Call produces one-shot Reply authority associated with a particular pending invocation. This enables delayed replies and authorized delegation without storing a single implicit reply slot in the server.

- Pending payload, badge, transfer metadata, and completion state belong to each blocked invocation. Multiple callers cannot overwrite one endpoint-global message.
- Both sender-first and receiver-first arrival paths implement the same rendezvous and reply-creation semantics.
- Define message label, data-word count, capability-transfer count, sender badge, and result representation together. The existing label-plus-five-words sketch is a proposal, not permission to truncate words to fit the ordinary transport.
- Specify whether Send is nonblocking/error-on-no-receiver as in the current wrapper or has another contract. Do not silently turn it into a blocking Call without a reply.
- Receive destinations and reply slots are explicit or reserved before committing delivery. A sender-local transfer slot is not a receiver-local handle.
- A reply is consumed exactly once at successful commit, or retired by explicit cancellation/teardown. Pre-commit errors retain ownership. Dropping a userspace wrapper cannot be the sole guarantee that a blocked caller is eventually released.
- ReplyRecv and Forward are follow-on operations, not prerequisites for correctness. ReplyRecv must distinguish reply-committed/receive-failed from failure before replying. Forward must identify and transfer the actual reply authority.

Preserve **open waits** (for any authorized peer on an endpoint) and **closed waits** (for a specific call/peer as defined by the protocol). Closed-wait identity must survive slot/domain reuse safely. Support separate send and receive/reply-phase timeout concepts; define no-wait, infinite wait, units, deadline clock, and cancellation races before selecting their wire encoding (D7). A timeout after request delivery cannot pretend to undo work already observed by the receiver; late replies have a defined disposition.

### Completion and scheduling boundary

An object transition can complete now, block, or request a handoff. Blocking is not an ordinary successful return followed by an ad hoc scheduler call. Save invocation/continuation state and eventual return values explicitly, and complete the userspace return only when the operation completes or is cancelled.

Scheduling and context switching occur after relevant object references and incompatible lock guards end. A fast direct switch is an optimization of that contract, not a bypass. IPC-related time donation must obey the Time accounting contract. Resource reservation for reply records and wait queues must be bounded and accounted, rather than hidden allocation inside an otherwise guaranteed rendezvous.

### Aborted-work vocabulary

Maintainer-adopted terminology (2026-09-05); these are semantic outcome categories, not new wire statuses or an approved result layout:

| Outcome | Meaning |
|---|---|
| Rejected before admission | This attempt did not start |
| Cancelled before commit | The operation guarantees its defined commit did not occur; this does not erase all possible preparatory effects |
| Completed | The outcome is known, including an operation-specific failure or explicitly reported partial result |
| Outcome unknown | Work may have committed, but the observer cannot establish completion |

Authority validity and operation outcome are separate: revoking authority does not prove that already-delivered work did not commit. A timeout or lost reply alone cannot be reported as cancellation-before-commit. The nucleus provides local invocation, cancellation, and completion mechanisms only. Distributed/remote protocols, retries, deduplication, durable receipts, and network failure policy belong in userspace services, not the kernel. D7/D9 must specify which categories each local operation can produce and their exact ABI representation.

## Time and userspace scheduling

Time is a first-class capability to a bounded CPU budget, not just a timer object. Userspace schedulers implement policy, distribute budget hierarchically, and observe DCBs. The nucleus enforces budget consumption, deadlines, preemption, and authorized transitions.

- **Donate** authorizes execution of a target using a defined budget and can suspend the donor until yield, exhaustion, cancellation, or another specified completion.
- **Split** creates a child budget in an explicit destination, reducing the parent's remaining amount only on commit.
- **Merge** combines compatible budgets without double counting; parent/provenance and deadline compatibility are checked.
- **Query** observes remaining budget through the same checked result convention as other operations.
- Expiry/revocation prevents further execution on retired budget and produces an explicit donor/parent continuation outcome. Reclamation accounts for already-consumed time.

The research intended unused time to return to the parent scheduler. Settle whether donation is a temporary loan or permanent transfer, where the remainder resides, and what deletion/yield means before exposing consuming Rust wrappers (D8). Memory-backed storage for Time does not mint positive budget; root budget issuance/replenishment requires explicit scheduler authority and conservation rules.

Time sketches use microseconds; DCB accounting uses nanoseconds. D8 must select wire/internal units, clock and rounding/overflow behavior. Nanosecond internal accounting is a recommendation, not an unannounced ABI change. Multiprocessor ownership and simultaneous donation must not permit spending the same budget twice.

## Implementation status and known migration gaps

Snapshot at initial consolidation (2026-09-05); update this table as complete slices land. Module inclusion and dispatch determine reachability, not file presence. Maintainer clarification: operation families marked excluded below are excluded because they do not compile as-is today; they remain intended functionality and must inform design and future implementation. Exclusion is not rejection of their design intent or permission to discard them.

| Family | Userspace | Nucleus API | Object/storage |
|---|---|---|---|
| DebugConsole | Debug-gated, explicit issued key; errors propagated | Debug-gated; full-width key/op decoding and slot-incarnation checks | Trusted-debug boot/SVC path tested; user-copy and rights isolation still unfinished |
| Domain | Included; mutation errors decoded losslessly, DCB access still incomplete | Excluded sketch; dispatcher reports unsupported | Included, partial lifecycle/storage |
| KeyTable | Packed selectors and destination-local results; `transfer` explicitly unsupported | Excluded sketch; dispatcher reports unsupported | Slot counters, checked lookup/removal, atomic insertion and exhaustion tested; object lifetime/management operations unfinished |
| Frame/architecture | Operation modules excluded | Handlers excluded; architecture dispatch unsupported | Inline regions and AArch64 scaffolding included; creation/invocation stubs |
| Untyped / Buffer | Excluded sketches | Excluded sketches | Excluded sketches; region payload helpers included separately |
| Time / Endpoint / Reply / Notification / EventCount | Excluded sketches | Excluded sketches | Excluded sketches |

KeyTable clients now encode complete keys for the invoked table, source selector and destination-table capability. CopyDerive/grant return the destination-local RawKey. `grant_to` still requests the prototype `Rights::all()` and Revoke remains an excluded legacy request, not an approved revocation mechanism. `transfer` now explicitly returns unsupported rather than false success. No KeyTable management handler is enabled; rights and guarded object storage remain prerequisites. Domain public handle construction remains deferred because its observation methods do not yet establish DCB mapping/lifetime safety.

The nucleus is inert at boot (2026-09-12): Kickstart builds the initial `Nucleus` in memory carved from a boot Untyped's unused watermark range, installs the boot Domain and initial grants (boot Untyped at `KeySlot::BOOT_UNTYPED`, debug console at `KeySlot::DEBUG_CONSOLE`), and records the carved address via the exported `nucleus_set_anchor` setter; the nucleus reads that anchor under a separate kernel lock on each syscall. The old lazy `NUCLEUS` fixture, `BOOT_DOMAIN_STORAGE`, and the `nucleus_bootstrap_debug_console` in-nucleus bridge are removed. No SVC bootstrap operation, slot-to-key refresh, or guessed incarnation is introduced. The nucleus object model is exposed as a lib so Kickstart can construct the identical `Nucleus` value. This is boot-ceremony carving, not implementation of general Untyped-backed pools, kernel-private KeyTable backing, or hostile-EL0 isolation. `just test-capability-boot` runs QEMU directly: in-guest assertions validate actual key handoff, successful debug Write through SVC, and exact zero-incarnation rejection. Semihosting exits with success after boot completes or failure on panic; no host log parser or recipe-level timeout is used.

Core-ID drift is now reconciled through the shared catalogues, with exhaustive raw/index conversion tests. Remaining cross-layer blockers include unmigrated excluded clients, unsafe general pointer-derived references, unchecked exception origins/user memory, DCB layout/reuse drift, independent object lifetime metadata, incomplete rights, inconsistent draft IPC registers, nontransactional allocation/transfers, and missing deferred completion. Slot-counter safety is implemented, not general object-generation/reclamation safety. They are tracked as work in the plan, not adopted as intended behavior.

There are no validated performance promises here. Measure supported paths after correctness and state what target/workload was measured.

## Decision register

Resolve the decisions needed by a slice before enabling it. An unrelated open decision need not block pure ABI tests or repairs to the active path. Record the chosen contract here, its rationale and compatibility impact, and then update the checklist. Do not infer approval from a suggested default.

| ID | Open decision and constraints | Needed before |
|---|---|---|
| D1 | Selected: hostile native code; explicit capability-scoped trust; Domain = VSpace protection boundary; userspace-only services initially; memory confidentiality/integrity; shared numerical meaning/cheap fbuf sharing with separate protected-context fallback; no intra-Domain frame aliases, cross-Domain aliases/writers allowed; DMA mediation/IOMMU; libOS availability policy; side channels deferred; authoritative revocation over borrows. Selected follow-up: fbuf addresses agreed before mapping; multi-node global allocation out of scope; stale raw-pointer prevention after VA reuse delegated outside the kernel for now. Open: machine-local namespace/reservation conflicts, per-target protection features, fault handling and Rust reference contracts. Far-future: stronger temporal-VA guarantees. | Domain protection and mapping semantics |
| D2 | Selected: explicit management authority carries bookkeeping responsibility and composition-scoped TCB membership; multiple/hierarchical userspace managers are policy; no mandatory central post-hoc registration. SPeCK-like kernel mechanisms retire objects and reject old invocations while manager cleanup can run in background; libOS orchestrates unmap before invalidation. Open: checked management-operation schemas, selective subtree invalidation of a still-live object, completion/handshake, and safe reuse enforcement. | General derivation/revoke and reclaiming retyped memory |
| D3 | Selected: caller-local slot/incarnation keys with implicit table context; initial 32-bit slot plus 32-bit incarnation (optional type would take 8 slot bits); constant-controlled fixed capacity, runtime resizing out of initial scope; independent object identity; slot-local exhaustion without wrap and with cleanup permitted; derive rather than mutate authority in place initially; Move yields a destination-local key; inconsistency with diagnostic reasons; enforced single-core guarded access, SMP deferred; Untyped-backed kernel-private KeyTables; no automatic leak recovery; correctness-first layouts; intended private MappedSlice cap. Selected follow-up: packed incarnation-high/slot-low 64-bit key, no initial type tag, increment-on-install from 1, retained counters on deletion, no live-Domain logical table rebinding, non-owning typed handles, statuses 26–28 with diagnostic reasons, and 64-bit kernel allocation generations without wrap. Concrete guarded access selected (2026-09-07): entries store pool tag + index + generation only (no raw pointers), per-pool authoritative slot metadata validated before dereference, per-invocation `!Send` access context with borrow-checked guard lifetimes, and explicit alias-rejecting pair resolution. Untyped-backed carve primitives and the boot Untyped allocation are established in Kickstart (2026-09-12); KeyTable backing and full pool/metadata ownership semantics remain Phase 5. Open: domain lifecycle integration, future in-place authority-mutation rules, Untyped-backed pool/metadata ownership (Phase 5), unsafe full-borrow/fbuf protocols, per-kind details, and interim Frame slot no-reuse scope. Stronger stale-raw-pointer protection across VA reuse is deferred. | General capability access, domain teardown, safe client ownership APIs |
| D4 | Selected: Retype origin caps carry delegable lifetime-control permission; donation preserves the creator's retained control; KeyTable management authority, not a special manager identity, enables direct derivation with entrusted bookkeeping. Separately grantable table permissions and incarnation-checked management operands follow the general authorization rule without manager-identity exceptions; minimal CopyDerive/Move/Delete schemas, vacant destinations and same-slot Move rejection are selected for KeyTable/debug-gated DebugConsole. Kickstart establishes initial Untypeds/KeyTables and delegates onward. DebugConsole remains opt-in `debug_kernel`, not generally available. The inert-nucleus handoff is selected (2026-09-12): Kickstart builds the initial `Nucleus` in carved memory and records it via `nucleus_set_anchor`; the nucleus reads the anchor under a separate kernel lock. Open: per-operation rights/bit assignments and origin-only remap enforcement, badges, exact bootstrap Domains/capacities/placement/grants and incarnation-bearing handoff records, notification indexing, and Domain.Grant relationship to KeyTable operations. | Exposing those authorities or bootstrap records |
| D5 | Selected direction: persistent DcbView. Open: DCB layout/stride, page sizing, visibility/discovery, enforceable backing availability, snapshots/publication, event summaries, and record reuse. | Stable userspace DCB observations |
| D6 | Selected Frame direction: Copy is not Map; each copied cap can Map the same frame at a different Domain/virtual page; Unmap is local and origin cap-Revoke withdraws descendants; origin remap remains a valid operation; full revoke/no Frame-slot reuse on deprovision is interim. Selected Retype invariant: only Untyped unused-watermark allocations with no outstanding access, not arbitrary live-object conversion or an allocation-time unmap protocol. Open: shared-namespace scope/conflicts and alias enforcement, precise Map/remap/revoke/no-reuse schemas and orchestration, three sharing-mode/fbuf protocols, Retype layout/accounting and enforcement of unused-watermark/no-outstanding-access allocation, sanitization, Buffer role, mapping identity, ASID/hardware-safe reuse and device rules. Stale raw-pointer temporal-VA protection is far-future work. | Memory-object vertical slice |
| D7 | Adopted aborted-work vocabulary and local-mechanisms-only kernel boundary; no new wire encoding. Open: IPC payload/transport, operation set, transfer/reply destinations and failure semantics, open/closed waits, timeouts, cancellation, deferred completion, notification delivery, shared-payload ordering/stability, and event-count overflow. | Blocking primitives and IPC activation |
| D8 | Budget issuance, donation loan/transfer, unused-budget return, split/merge/deletion/expiry, units/clocks, multicore accounting. | Time/scheduler vertical slice |
| D9 | DebugConsole gate preserves type `127` and Write `0`; no new inline-byte ABI is approved. The approved key package specifies packed keys, statuses 26–28, diagnostic reasons/precedence, CopyDerive register packing and coordinated rebuild with no slot-only fallback. Minimal KeyTable logical schemas are selected; Move/Delete's remaining wire details are pending. Aborted-work terms are adopted vocabulary only. Wire encodings, other shared errors/details, reserved/unsupported operations, ABI version/support discovery, and migration policy remain open. | Freezing new operation schemas or separately deployed ABI consumers |

## Definition of a supported operation

A supported operation has one shared schema; a client that preserves results; checked nucleus decoding and authorization for every participating resource; typed state transitions with explicit ownership/failure semantics; documented blocking and teardown behavior; and validation at the appropriate ABI, model, and target-integration levels.

Safety comments describe actual invariants and their owner, not just that a call is unsafe. Layout and round-trip assertions remain enabled. Tests include malformed and adversarial requests, aliasing/identity reuse, capacity failures, cancellation, and rollback where applicable.

Follow the [implementation plan](capabilities_implementation_plan.md) in small dependency-respecting slices. The repository skill at `.claude/skills/capability-refactor/SKILL.md` describes the working procedure; it does not replace the contracts in this document.
