# Object types

User-facing reference for every object kind in the Vesper capability catalogue,
one document per kind. Each document follows the same structure: **name**,
**purpose**, **user-level visible operations**, **kernel-level implementation
details**, **sidenotes**, **TODOs**, and a final cross-reference section
comparing the current implementation against the desired-capability notes in
the `🧠 Vesper` vault.

Contract sources: [`doc/nucleus_capabilities.md`](../nucleus_capabilities.md)
(design contracts and decision register), [`doc/lifetime-and-authority.md`](../lifetime-and-authority.md)
(lifetime/authority semantics), [`doc/capabilities_implementation_plan.md`](../capabilities_implementation_plan.md)
(slice plan and validation status).

## Wire encoding

The wire object type is one byte. Bit 7 selects the category; bits 6–0 hold the
category-local index.

- Core: `wire_type = core_index`, range `0x00..=0x7f`.
- Architecture: `wire_type = 0x80 | arch_index`, range `0x80..=0xff`.

Unknown and reserved indices fail explicitly with a defined error; a reserved
ID never advertises implementation support.

## Catalogue

### Core kinds (`core/`)

| Kind | Wire | Document | Status |
|---|---:|---|---|
| Null | `0x00` | [null.md](core/null.md) | Active (always rejected) |
| Untyped | `0x01` | [untyped.md](core/untyped.md) | Retype active |
| KeyTable | `0x02` | [key_table.md](core/key_table.md) | AddressSpace-associated caller lookup; CopyDerive/Move/Delete active |
| Thread | `0x03` | [thread.md](core/thread.md) | Named Thread Retire active; Return `0` only on `CurrentReturnOnly` (sentinel installed at Slot(1) by AddressSpace provisioning), active through real SVC dispatch with the `ThreadReturnKey::return_from_invocation` helper; underflow/retired-source Return faults halt the kernel under the interim policy (fault delivery D1/D7); Thread-resident wait continuations and inline depth-16 invocation stack; checked root/ASID switching on a shared trap stack |
| Time | `0x04` | [time.md](core/time.md) | Excluded sketch |
| Scheduler | `0x05` | [scheduler.md](core/scheduler.md) | Contract selected; not implemented |
| Brand | `0x06` | [brand.md](core/brand.md) | Contract direction recorded; implementation deferred to IRQ |
| Invocation | `0x07` | [invocation.md](core/invocation.md) | Call-only `0` with mandatory `NonZero<u64>` function address. Active: six-input construction with immutable validated extent/headroom (40 B payload); `InvalidStack` 32 (reasons 1–12), `NestingDepth` 33 and synthetic `UnexpectedReturn` 34; Call admission (key/op/`CALL` → live target → saved-x9 SP → translation → depth), commit and entry-side install/restore/trace through real SVC; kernel-owned x19–x30 preservation, GPR/NZCV scrubbing and saved-source SPSR inheritance; `InvocationKey::call`, `ppc_export!` wrapper, 16-byte result spill and `vesper_thread_return_fault` handoff; validated by same-Thread Call/Return into Bounce (kicktest) and by a client/endpoint/server rendezvous across three AddressSpaces that blocks inside the endpoint (endpoint-test). Provisional/experimental: x9 SP transport and two-u64 body-return convention (freezing either is a pending maintainer decision). Not implemented: Return fault delivery (interim kernel panic), effective FP/SIMD trapping, TLS/debug/complete state isolation, per-call time attribution, Invocation distribution |
| Notification | `0x08` | [notification.md](core/notification.md) | Signal/Wait/Poll active |
| EventCount | `0x09` | [event_count.md](core/event_count.md) | Advance/Await/Read active |
| DebugConsole | `0x7f` | [debug_console.md](core/debug_console.md) | Debug-gated Write |

IDs 10–126 are reserved. There is no `Buffer` wire kind (Buffer is a
userspace/libOS construct over frame capabilities) and no `Domain` wire kind
(the execution entity is the core `Thread`; the mapping context is the arch
`AddressSpace`).

### Architecture kinds (`arch/`)

| Kind | Wire | Document | Status (AArch64) |
|---|---:|---|---|
| Frame | `0x80` | [frame.md](arch/frame.md) | Map/Unmap/GetAddress active; non-global TTBR0 leaves and ordered VA-scoped invalidation |
| PageTable | `0x81` | [page_table.md](arch/page_table.md) | Map/Unmap active; ordered whole-ASID root invalidation |
| AddressSpace | `0x82` | [address_space.md](arch/address_space.md) | Immutable KeyTable binding/current-AS caller lookup; Activate installs after guards end; Retire/CreateInvocation active; six-input construction stores validated extent/headroom (`x5` base, `x6` exclusive end, `x7` minimum headroom in bytes), with status 32 diagnostics and authority/live-target → nonzero entry → stack predicates → destination admission; AArch64 user ceiling `1 << 48`; Invocations it creates are callable through dispatched PPC Call |
| ASIDPool | `0x83` | [asid_pool.md](arch/asid_pool.md) | Assign active (boot-provided); the test kernels bind ASIDs 1–3; full pool/hardware-width reconciliation pending |
| ASIDControl | `0x84` | [asid_control.md](arch/asid_control.md) | Reserved |
| IOSpace | `0x85` | [io_space.md](arch/io_space.md) | Deferred |
| IOPort | `0x86` | [io_port.md](arch/io_port.md) | x86-only; unsupported on AArch64 |
| IRQHandler | `0x87` | [irq_handler.md](arch/irq_handler.md) | Deferred |
| IRQControl | `0x88` | [irq_control.md](arch/irq_control.md) | Deferred |

Indices 9–127 are reserved. Support is target-specific: a registered ID (for
example `IOPort`) does not become supported on AArch64 merely because it is
defined.

## Invocation model

All operations are invoked through a single `SVC #0` capability invocation.
Entry registers: `x0` packed caller-local key (incarnation in bits 63–32, slot
in bits 31–0), `x1` operation number, `x2..x7` six operation arguments.
Results: `x0` status (zero = success), `x1`/`x2` two result words (ordinary
contract). Unless an operation explicitly ignores them, unused argument words
are transmitted as zero and rejected when nonzero (strict wire-argument
convention). `Thread.Return` ignores x4..x7 without userspace initialization
or zeroing; successful resumed-frame scrubbing does not change that input rule.

Invocation is Call-only `0` with a mandatory `NonZero<u64>` function address.
`Thread.Return` is `0` only on the explicit `CurrentReturnOnly` Thread selector,
never `Named(ObjectId)`. `KeySlot::THREAD_RETURN` remains Slot(1); all other
slots and kind IDs are unchanged. The sentinel names no function, AddressSpace
or concrete Thread, carries no Thread-management rights, and rejects
management operations and `object_id` extraction. Named Thread entries reject
Return with `InvalidOperation`. It uses ordinary live-current-AS/table/SELF,
guard, incarnation, bounds and presence checks, not a lookup bypass or magic
raw key. All Threads in an AddressSpace share the sentinel, but it acts only
on the invoking Thread's own top continuation; AddressSpace provisioning installs it before activation.
Opcode zero on an Invocation means Call, not wrong-form Return. Unknown
Invocation opcode `1` is `InvalidOperation` from the active Call handler.
Call and Return dispatch with same-Thread PPC migration are implemented; Return
faults halt the kernel under the interim policy. The userspace
`InvocationKey::call` wrapper, `ThreadReturnKey` helper and `ppc_export!`
export adapter are implemented. `ThreadSelector`, its kernel
constructors/accessors, `ThreadOp::Return = 0` and boot Slot(1) installation are
implemented. Current-relative `object_id` extraction returns `InvalidOperation`,
and Retire rejects the sentinel before checking rights or extracting identity.
Return through a named Thread returns `InvalidOperation`. AddressSpace provisioning installs the Return sentinel, so it is not propagated
through KeyTable management. Call-only Invocation distribution remains deferred,
with no broader derivation or transfer permission.

```mermaid
flowchart TD
    A["SVC #0 entry"] --> B["Validate exception class,<br/>SVC immediate, origin"]
    B --> AS["Validate current Thread's AddressSpace identity"]
    AS --> KT["Resolve bound KeyTable; validate SELF address and capacity"]
    KT --> C["Lookup invoked key with SELF-sourced guard"]
    C --> D{"Arch bit set?"}
    D -- "core" --> E["core_invoke"]
    D -- "arch" --> F["arch_invoke"]
    E --> G{"CoreType match"}
    G --> H["Untyped / Thread (incl. Return) / KeyTable /<br/>Invocation (Call) / Notification /<br/>EventCount / DebugConsole (debug_kernel)"]
    G --> I["Null → NullCapability"]
    G --> J["Time / Scheduler / Brand →<br/>UnsupportedCoreType"]
    F --> K{"ArchType match"}
    K --> L["Frame / PageTable / AddressSpace /<br/>ASIDPool"]
    K --> M["ASIDControl / IO / IRQ →<br/>UnsupportedArchType"]
```

Blocking operations do not return a "blocked" status. The bounded wait/resume
path validates the selected Thread's live AddressSpace, root/ASID encoding,
saved execution state, and pending completion before committing park/select.
The caller's continuation resides in Thread storage. Entry installs the copied
root/ASID after object/Access guards and the kernel lock end, restores the
transient frame with the terminal result, and returns normally through `ERET`
on the shared per-core kernel stack. Rejected scheduling preparation preserves
Thread contexts, pending records, current selection, and the runnable FIFO;
it does not undo the wait already admitted by capability dispatch. The trusted
fixture treats an impossible preparation failure as an invariant failure, not
wait completion or a recovery ABI.

Source and Bounce use independently provisioned roots with ASIDs 1 and 2;
source activation precedes the first handoff. Subsequent capability invocations
resolve the selected Thread's current AddressSpace's keytable, not an
independent table selector. Execution is trusted EL1t on SP_EL0 with a shared
high SP_EL1 trap stack, not same-Thread PPC migration or protected EL0
confinement. Prepared metadata does not pin backing: immediate installation
relies on the serialized single-core, masked, non-reentrant trap interval;
later or asynchronous use requires fresh validation.

`AddressSpace.Activate` likewise prepares checked metadata and installs it at
syscall entry after guards and the kernel lock end, before tracing success and
returning zeros. Its internal deferred outcome adds no wire status or ABI.
The supported AArch64 profile is an L0-rooted, four-level, 48-bit TTBR0 walk
(`T0SZ=16`, `EPD0=0`, `A1=0`, 4 KiB granule, no DS/LPA2), with configured and
hardware PA/ASID checks — see [AddressSpace](arch/address_space.md).

Success and operation-specific error wait completions are active;
cancellation-resume encoding remains open — see the completion foundation in
`doc/nucleus_capabilities.md` § "Communication and deferred completion".

### PPC construction and storage

`AddressSpaceKey::create_invocation` appends `stack_base`, `stack_end`, and
`minimum_headroom` to its function/destination/slot inputs and uses ordinary
`protected_call6`. The kernel stores an immutable validated
`InvocationStackExtent` inline with the target identity and nonzero entry.
Call admission applies its numeric `validate_sp` helper to the saved `x9`
through the dispatched Call path. Construction does not
prove mapped/writable stack backing or executable-entry validity. Statuses
33/34, invocation-stack storage, Call/Return, PPC wrappers/adapters, faults and
distribution remain deferred.

The current `InvocationPayload` and `KeyPayload` are 40 B; `KeyEntry` is 64 B
with alignment 32. KeyTables retain a 32 B header and separate 4 B incarnation
counters: `KeyTable::carve_size(8)` is 17,440 B for 256 entries. Runtime Retype,
bootstrap, archive tables and fixture backing use type-derived carve sizes;
entry growth does not change packed key widths, guards or slot-incarnation
semantics.

### PPC migration-frame contract

The [Invocation GPR/NZCV policy](core/invocation.md#non-payload-gpr-and-condition-flag-exposure)
is implemented and validated through real Call/Return. On successful Call, the kernel first
saves source x19..x30/context and consumes provisional target SP from saved
`frame.gpr[9]`; it retains real arguments x2..x7, zeroes dummy x0/x1 and
x8..x30, clears target NZCV, inherits source saved SPSR mode/masks/other
non-NZCV controls, and sets target execution SP/PC. Status comes from the
saved admitted source frame, not live kernel-handler PSTATE; EL1t stays EL1t
and EL0 stays EL0. No source-side
dummy-zeroing is required. Successful Return captures r0/r1 before rewriting
the frame, delivers x0=SUCCESS/x1=r0/x2=r1, zeroes x3..x18, and restores exact
source x19..x30, AddressSpace/SP/PC/origin and raw SPSR including NZCV.
Migration scrubbing does not apply to recoverable local Call/Return rejection;
existing preservation/error contracts are unchanged.

This requires no extra fields or runtime allocation; the continuation
record/depth-16 array sizes are 144 B/2304 B. Only GPR/NZCV disclosure is
addressed, together with implemented saved-source status inheritance. TLS, debug state and complete architectural-state
isolation remain open; effective FP/SIMD trap enforcement is separate.
Trusted EL1 tests do not establish hostile-EL0 confinement. x9 remains
provisional, and the native two-u64 body-result convention remains experimental.

## Cross-reference note

The final section of each document compares the current implementation with
the desired-capability notes in the `🧠 Vesper` vault (Obsidian). Those notes
are research intent, not implemented contracts; discrepancies listed there are
for maintainer review, not adopted behavior.
