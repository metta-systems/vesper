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

The research direction is a **single shared virtual-address namespace**, supporting cheap sharing and avoiding unnecessary address-space switching. Preserve this goal; do not silently replace it with a conventional process model merely because VSpace types exist in the code.

A shared namespace is not itself an isolation mechanism. Capability checks mediate nucleus operations, not arbitrary CPU loads/stores after a mapping is installed. MTE and PAC alone do not establish robust isolation between mutually distrustful domains; realms and other hardware mechanisms have their own constraints. The actual protection mechanism, threat model, mapping permissions, and architecture-specific fallback must be chosen explicitly (D1).

VSpace represents translation/protection context where the backend requires it, conceptually composing a root page table and ASID binding. Whether contexts share one virtual namespace, one translation root, or multiple protected views is part of D1/D6. Domain IDs must not be assumed to be VSpace IDs.

Mapping the same physical Frame at different virtual pages in different Domains is intended use of derived Frame capabilities. How those aliases coexist with the shared-address-space goal remains D1/D6: a shared address namespace need not mean one virtual address per frame or one page-table root. See the alternatives in [Lifetime and authority semantics](lifetime-and-authority.md). Selecting SPeCK-like resource mechanisms does not itself choose a translation/protection model.

Bootstrap is explicit: the initial privileged component receives authority over discovered resources and delegates subsets to resource managers and child domains. Well-known slots are conventions for locating granted capabilities, never a way to manufacture them. Debug-console authority is an explicit bootstrap/delegation choice, not an entitlement implied by knowing its slot.

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
- **`Key<T>` / typed key wrapper**: names the particular capability incarnation obtained by the caller, not a slot's future occupant (maintainer decision, 2026-09-05). Altered or invalidated authority must fail invocation with an explicit inconsistency error. The current slot-only wrapper does not yet implement this guarantee; incarnation encoding and the error's wire schema remain D3/D9. Constructing or copying a Rust wrapper does not mint kernel authority or perform capability Copy. Normal authorized resource operations need not change capability incarnation; Frame remapping is explicitly such an operation, with detailed mapping semantics below.
- **`KeyEntry`**: a fixed-size kernel capability value containing object kind, rights, badge/other authority metadata, and an object handle or inline region description. It is not a userspace record.
- **Kernel object/resource**: persistent state with a separately managed lifetime. Multiple entries may refer to it when its contract permits.
- **`DomainId`**: identity for domain state and observation, not a capability granting control. Reuse must not let stale references accidentally identify a new domain.
- **Owned slot, mapping, or reply**: a stronger wrapper only where the runtime and kernel can enforce its ownership contract. Do not infer ownership from a plain `Key<T>`.

Cross-domain operations identify the destination through authority over the destination table/domain, not by reinterpreting a sender-local slot in the receiver's table. A destination slot is explicit or reserved through a documented protocol.

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

For the current AArch64 control-call transport:

| Direction | Registers | Meaning |
|---|---|---|
| Entry | `SVC #0` | Capability invocation |
| Input | `x0` | Caller-local capability slot |
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

`syscall_status` in `libs/object/src/syscall_status.rs` defines the shared symbolic wire statuses: success zero and the existing errors 1–25. `CapError::code()` and `decode_syscall_result` use the same error constants; nucleus syscall return handling uses `SUCCESS` and `CapError::code()`. Preserve these status meanings; literal ABI tests independently pin their numeric values. Correct object-type details to the canonical numbering above. Make clear which error detail is a category-local index and which is a full wire ObjectType.

Do not create competing per-family wire error spaces such as the draft `RetypeError`. Typed client errors may wrap the shared decoded result. Unknown future status/detail values must remain observable without panicking or being turned into success. New errors for inconsistency, overflow, cancellation, unsupported behavior, and partial completion require explicit shared definitions, not ad hoc sentinel values (D9). Inconsistency for a stale/altered capability incarnation is an approved semantic requirement, not yet a numeric status or implemented `CapError` variant.

The shared `decode_syscall_result` now decodes the existing three-word result without changing this wire baseline. Zero status preserves both success words; statuses 1–25 decode to their existing variants only when detail widths/known kind indices are representable and unused words are zero. Otherwise `CapError::UnknownResponse` preserves the nonzero status and both details verbatim, including future extensions to known errors. This is a client representation, not a new status or a kernel reserved-argument rule. Domain mutation wrappers and the four syscall-backed KeyTable wrappers (`copy_derive`, `delete`, `revoke`, `grant_to`) use it; other families remain unmigrated. The added Rust enum variant requires downstream exhaustive matches to be updated, but existing wire meanings and Domain/KeyTable method signatures are unchanged.

Kernel and userspace changes to IDs, layouts, and meaning are coordinated migrations. Before separately versioned components are supported, choose ABI compatibility/version discovery and how callers learn which optional operations are available (D9). No performance claim or `repr` annotation substitutes for a layout/round-trip test.

### Object operation baseline

The following preserves existing operation declarations as a starting vocabulary. Except for console write, it is not a claim of end-to-end support. Entries labeled deferred or unresolved require contract decisions before activation; do not silently reuse their numbers.

| Family | Existing operation IDs / intended vocabulary | Contract qualification |
|---|---|---|
| Null | None | Never a usable capability |
| Untyped | Retype `0` | Explicit destination; single versus batch request shape remains D6 |
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

Implementation status: `KeyTableOp` provides checked `TryFrom<u64>` and `TryFrom<u32>` decoding of the existing IDs above. The full-width decoder rejects every other value with the existing `InvalidOperation` error, including unassigned `3` and high-bit aliases; the `u32` adapter widens before delegating. This is shared ABI vocabulary only: the excluded KeyTable handler is not enabled, active dispatch still reports `UnsupportedCoreType(KeyTable)` after successful lookup, and entry-level register validation remains unfinished. No rights, slot/argument schema, ownership, or D2–D4/D9 decision is implied.

### DebugConsole debug-only exception

Maintainer decision (2026-09-05, scoped D4/D9): DebugConsole is **not a generally available capability**. Its handler, bootstrap grant, userspace wrapper, and boot demonstration require the opt-in Cargo feature `debug_kernel`, disabled by default. This feature identifies a debug kernel independently of Cargo's optimization profile: the embedded build recipes use `--release` even for debugging. Neither `qemu` nor `jtag` implicitly enables it. Build nucleus and kickstart together, for example with `just build rpi3 qemu,debug_kernel`; production kernels must omit `debug_kernel`.

Keep the current pointer-based Write mechanism for trusted debugging only; do not add an inline-byte operation or change the wire schema in this slice. The canonical type ID `127` and operation ID `0` remain reserved/defined regardless of feature availability. Without the kernel feature, no console capability is installed and DebugConsole dispatch is unsupported (an absent slot still returns the normal lookup error).

Temporary debug-only leeway is not a safety or isolation guarantee. The existing EL1 bootstrap caller, domain-zero fallback, broad grant, unchecked physical/direct-map pointer interpretation and copying, C-string limitations, and discarded client errors remain migration gaps. The stateless console handler now validates the capability's type through a shared entry borrow without deriving an object reference from its payload; this removes the console-specific mutable casts, not the general storage/lifetime gaps. Actual console output currently requires `qemu`; enabling `debug_kernel` alone does not add a hardware output backend. Deferred improvements and alternatives are recorded beside `kernel/nucleus/src/api/debug_console.rs::invoke`. General availability requires the normal supported-operation criteria, including explicit caller/rights, bounded caller-authorized memory access with fault recovery, defined byte/length/partial-output semantics, checked exception/argument decoding, and observable results. D1/D3/D4/D6/D9 remain open beyond this limited availability decision.

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

### Maintainer lifecycle and authority decisions (2026-09-05)

- `Untyped.Retype` yields creation-origin capabilities carrying permission to control the created object's lifetime. That permission may be delegated in derived capabilities; retirement authorization follows capability permissions, not a separate privileged owner identity. Delegating/donating object authority does not implicitly surrender the creator's control: it retains its authority until it destroys its own capability. This does not resolve the separate consuming CPU-budget semantics of Time.Donate (D8).
- Deleting the last retirement-authorized capability need not retire the object. Correct resource management is the OS's responsibility. A resulting permanently lost retyped allocation is an accepted leak, not a kernel obligation to recover or run an automatic final-capability destructor. Leaked storage remains unavailable for conflicting reuse.
- Trusted userspace KeyMaster is solely responsible for derivation-tree management. It orchestrates subtree revocation as a separate, potentially background process; the kernel supplies object retirement and checked capability/mapping mechanisms. The maintainer selects a SPeCK-like model for this division: userspace resource policy/derivation and kernel liveness, individual resource operations, and quiescence/reuse mechanisms. Exact metadata and synchronization protocols remain to be designed, not copied wholesale from Composite. How all derivation/transfer paths are restricted to or registered with KeyMaster remains to be implemented (D2/D4).
- Derived capabilities to one object refer to that same object, not successively nested kernel objects. Invalidating its authoritative lifetime identity/generation must reject all old capability invocations irrespective of table or tree depth. Object retirement triggers KeyMaster subtree cleanup, which need not synchronously erase every dead slot. It does not require a kernel ancestry walk just to detect retirement of that shared object.
- An object-wide generation cannot selectively invalidate one branch while retaining other capabilities to the same object. The mechanism and completion contract for selective subtree revocation without object retirement remain open (D2); do not impose kernel ancestry metadata or claim object generations solve this different operation.
- The library OS is responsible for correct unmap-before-invalidation orchestration. Invalidating capability authority before withdrawing installed access is a resource-management error in that trusted layer, not a requirement for a recipient to cooperate after invalidation. Kernel mapping primitives supply hardware transitions; the exact premature-reuse checks, retirement prerequisites, and manager/kernel completion handshake remain D2/D6. System-wide safe reuse still requires withdrawal of stale mappings and in-flight access.

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

Retype validates source authority, memory kind/device restrictions, absolute physical alignment, size representability, free capacity, destination authority, and destination vacancy before commitment. Watermark encoding cannot discard sub-alignment allocations. Choose single-object versus batch semantics explicitly; a batch needs a stated all-or-nothing or partial-result contract (D6).

Before freshly allocated or recycled ordinary RAM becomes observable across a protection boundary, the allocation protocol must guarantee initialization/sanitization so a new owner cannot read a prior owner's data or kernel metadata. Intentional content-preserving delegation/sharing is distinct from fresh allocation. Device memory requires its own policy and must not be blindly zeroed. D6 determines who performs sanitization and how completion is enforced.

Resource creation, split, copy, mapping, and transfer follow the conceptual sequence **validate → reserve → initialize/prepare → commit**. Recoverable failure before commit leaves source accounting and authority unchanged. If hardware or other irreversible work makes that impossible, expose and retain a recoverable partial state rather than losing bookkeeping.

### Mapping and sharing

Mapping identity must contain enough information to locate and retire the real mapping: translation/protection context, virtual address/range, permissions/attributes, and lifetime identity as required by the backend. A compressed virtual address without context is not sufficient for arbitrary VSpace mappings. Avoid unexplained exclusions such as address zero or a 44-bit-only range caused by a storage shortcut.

Mapping permissions are bounded by backing and target-context authority; cache/device attributes are a separate validated dimension. Record a mapping only with the actual hardware transition, and preserve enough state to roll back or finish partial map/unmap failures. Reuse requires completed hardware invalidation, including remote TLB or device translation synchronization where applicable.

Maintainer Frame clarification (2026-09-05): Copy and Map are separate. Unmap is mapping-local for origin A as well as derived B. The earlier phrase “origin Unmap” meant **capability Revoke on the origin**, not a stronger Frame.Unmap primitive. In the seL4-like distinction, origin Revoke withdraws descendant capabilities/mappings while retaining the origin; removal of the origin's own mapping/capability is separate, and object retirement remains a distinct permission-authorized operation. KeyMaster/libOS orchestrates the selected revocation semantics using kernel mechanisms; this is not approval of a kernel-managed derivation tree.

The same physical Frame may be mapped by different derived caps at different virtual pages in different Domains. These are normally distinct PTEs, not competing ownership of one PTE. Origin-authorized remapping remains a valid operation and does not by itself make the capability inconsistent; initial Map of a copied cap is not an origin-only remap. Whether remap changes only virtual placement or can replace physical backing, how descendant mappings respond, and how to encode/enforce origin-only remap authority remain open (D4/D6). Delegated retirement permission must not be treated as permission for a derived capability to remap.

Interim deprovisioning direction: fully revoke the Frame capability and do not reuse that slot for Frames. The precise scope/duration of this no-reuse restriction, exhaustion handling, and interaction with eventual incarnation-safe reuse remain D3/D6; it is not a replacement for object identity or hardware-safe backing reuse.

ASIDs come from an authoritative namespace with binding and safe reuse rules. Decide whether ASID capabilities are explicit resources or pool-owned VSpace bindings; preserve the registered IDs while this is unresolved. IRQ/I/O authority likewise comes from authorized hardware-resource assignment, not arbitrary retype.

Large-data IPC uses shared backing and explicit producer/consumer ownership protocols. Ring/buffer pools plus produced/consumed event counts support backpressure without allocation or copying in the hot path. A common address namespace can simplify sharing but is not required to equate authority with an address.

Safe userspace mapping guards must prevent independent unmap/revoke from leaving usable safe references. Intended MappedSlice ownership: create/own a private capability and its mapping, expose no further derivations or independently usable management aliases, and unmap on Drop. Cleanup uses the incarnation-bearing capability key and required kernel checks, so a stale guard must be rejected rather than affect a replacement. This is not yet implemented by the current wrapper, which borrows an existing BufferKey. Specify integration with ancestor revocation/object retirement so the private mapping remains valid for the borrow; private slot ownership does not by itself imply exclusive access to shared physical contents.

Ordinary Rust slices require additional guarantees against aliasing and external mutation. Shared memory, DMA, and MMIO need access protocols appropriate to their semantics, not automatic `&mut [u8]` creation from WRITE rights. Rust mutability versus shared-resource semantics remains open, including whether reference-producing APIs need to be unsafe. Buffer as a kernel object versus a userspace aggregate remains D6; do not remove its public kind merely as cosmetic cleanup.

## Domain and shared DCB contracts

A domain combines kernel-private execution/protection/capability state with a userspace-observable DCB. The kernel-private part is never exposed by mapping DCB pages. Domain creation establishes one identity across private state, DCB, keytable ownership, scheduler relationships, and any protection-context binding.

Domain identity lookup checks validity and allocation, not just whether a page exists. Invalid or stale IDs fail without indexing outside storage. Domain teardown retires queued calls, waits, replies, time donations, and other references before reuse. No current domain is an explicit state; it must not silently grant access to domain zero.

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
| DebugConsole | Only with `debug_kernel` | Only with `debug_kernel`; otherwise unsupported | Debug-only prototype; pointer/length/authority/error handling still needs repair |
| Domain | Included; mutation errors decoded losslessly, DCB access still incomplete | Excluded sketch; dispatcher reports unsupported | Included, partial lifecycle/storage |
| KeyTable | Included; syscall-backed mutation errors decoded losslessly; `transfer` remains a no-op placeholder | Excluded sketch; dispatcher reports unsupported | Included, partial lifecycle/storage |
| Frame/architecture | Operation modules excluded | Handlers excluded; architecture dispatch unsupported | Inline regions and AArch64 scaffolding included; creation/invocation stubs |
| Untyped / Buffer | Excluded sketches | Excluded sketches | Excluded sketches; region payload helpers included separately |
| Time / Endpoint / Reply / Notification / EventCount | Excluded sketches | Excluded sketches | Excluded sketches |

KeyTable's client-result repair preserves its existing request encoding, including `grant_to` requesting `Rights::all()` through CopyDerive and `revoke` invoking on `self` with its legacy table argument unused. It does not approve a rights, destination, or revocation schema. No KeyTable handler or lifecycle transition is enabled; D2–D4 remain prerequisites for that work. The no-op `transfer` placeholder and missing public handle construction remain deferred interface gaps.

Core-ID drift is now reconciled through the shared catalogues, with exhaustive raw/index conversion tests. Remaining cross-layer blockers include discarded client errors, unsafe pointer-derived references, unchecked syscall inputs, DCB layout/reuse drift, table occupancy bypasses, incomplete rights, inconsistent message registers, nontransactional allocation/transfers, and missing deferred completion. They are tracked as work in the plan, not adopted as intended behavior.

There are no validated performance promises here. Measure supported paths after correctness and state what target/workload was measured.

## Decision register

Resolve the decisions needed by a slice before enabling it. An unrelated open decision need not block pure ABI tests or repairs to the active path. Record the chosen contract here, its rationale and compatibility impact, and then update the checklist. Do not infer approval from a suggested default.

| ID | Open decision and constraints | Needed before |
|---|---|---|
| D1 | Shared-address-space protection model, threat model, backend isolation/fallback, and meaning of VSpace versus Domain. Preserve shared-namespace intent without claiming capabilities/MTE/PAC alone isolate arbitrary memory access. | Domain protection and mapping semantics |
| D2 | Selected: SPeCK-like resource mechanisms with trusted userspace KeyMaster managing derivation trees; kernel retires objects; shared-object invalidation rejects old invocations and tree cleanup can run in background; libOS orchestrates unmap before invalidation. Open: manager exclusivity enforcement, selective subtree invalidation of a still-live object, completion/handshake, and safe reuse enforcement. | General derivation/revoke and reclaiming retyped memory |
| D3 | Selected: keys name capability incarnations; stale/altered authority yields inconsistency; no automatic leak recovery; correctness-first field/layout changes; intended MappedSlice privately owns an underived mapping cap with checked cleanup. Open: identity encoding/wrap, guarded access/synchronization, borrow versus ancestor-retirement guarantees, Rust mutability, per-kind details, and interim Frame slot no-reuse scope. | General capability access, domain teardown, safe client ownership APIs |
| D4 | Selected: Retype origin caps carry delegable lifetime-control permission; donating object authority does not surrender the creator's retained control. DebugConsole remains opt-in `debug_kernel`, not generally available. Open: per-operation rights/bit assignments and enforcement of origin-only remap distinct from delegated retirement, badges, bootstrap/manager identity, notification indexing, and Domain.Grant relationship to KeyTable operations. | Exposing those authorities or bootstrap records |
| D5 | Selected direction: persistent DcbView. Open: DCB layout/stride, page sizing, visibility/discovery, enforceable backing availability, snapshots/publication, event summaries, and record reuse. | Stable userspace DCB observations |
| D6 | Selected Frame direction: Copy is not Map; each copied cap can Map the same frame at a different Domain/virtual page; Unmap is local and origin cap-Revoke withdraws descendants; origin remap remains a valid operation; full revoke/no Frame-slot reuse on deprovision is interim. Open: shared-namespace alias model, precise Map/remap/revoke/no-reuse schemas and orchestration, Retype layout/accounting, sanitization, Buffer role, mapping identity, ASID/reuse and device rules. | Memory-object vertical slice |
| D7 | Adopted aborted-work vocabulary and local-mechanisms-only kernel boundary; no new wire encoding. Open: IPC payload/transport, operation set, transfer/reply destinations and failure semantics, open/closed waits, timeouts, cancellation, deferred completion, notification delivery, shared-payload ordering/stability, and event-count overflow. | Blocking primitives and IPC activation |
| D8 | Budget issuance, donation loan/transfer, unused-budget return, split/merge/deletion/expiry, units/clocks, multicore accounting. | Time/scheduler vertical slice |
| D9 | DebugConsole gate preserves type `127` and Write `0`; no new inline-byte ABI is approved. Inconsistency is required for stale/altered capability incarnations; aborted-work terms are adopted vocabulary only. Wire encodings, other shared errors/details, reserved/unsupported operations, ABI version/support discovery, and migration policy remain open. | Freezing new operation schemas or separately deployed ABI consumers |

## Definition of a supported operation

A supported operation has one shared schema; a client that preserves results; checked nucleus decoding and authorization for every participating resource; typed state transitions with explicit ownership/failure semantics; documented blocking and teardown behavior; and validation at the appropriate ABI, model, and target-integration levels.

Safety comments describe actual invariants and their owner, not just that a call is unsafe. Layout and round-trip assertions remain enabled. Tests include malformed and adversarial requests, aliasing/identity reuse, capacity failures, cancellation, and rollback where applicable.

Follow the [implementation plan](capabilities_implementation_plan.md) in small dependency-respecting slices. The repository skill at `.claude/skills/capability-refactor/SKILL.md` describes the working procedure; it does not replace the contracts in this document.
