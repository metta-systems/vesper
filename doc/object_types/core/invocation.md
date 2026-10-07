# Invocation

| | |
|---|---|
| Wire type | `0x07` (core) |
| Target | An exported component API entry point in an `AddressSpace` |
| Status | Six-input construction, status 32 diagnostics and immutable validated stack extent/headroom are active; `Call` is active through real SVC dispatch: admission/preparation (`prepare_call`), commit (`commit_call`: continuation push, AddressSpace migration, scrubbed target-entry frame), and entry-side install, frame restore and success trace; same-Thread Call/Return round trip validated in the boot fixture; `InvocationKey::call` wrapper and `ppc_export!` export adapter implemented |

## Purpose

An `Invocation` capability identifies an entry point in a component API within
a target `AddressSpace` incarnation, with a mandatory `NonZero<u64>` function
address and only `CALL` authority. It does not name a server `Thread`; the
source `Thread` migrates into the target `AddressSpace`. Any component with
`GRANT` authority on the target `AddressSpace` and `INSTALL` authority on a
destination `KeyTable` can construct an `Invocation` through
`AddressSpace.CreateInvocation`. The target component/export setup chooses
and publishes a nonempty user-VA byte extent `[base, end)` with the Invocation,
using `x5` base, `x6` exclusive end, and `x7` positive minimum headroom as a
direct `u64` byte count at construction (not a size exponent);
`base >= end` is rejected. This allows it to account for stack-pool and
concurrency policy. At Call require 16-byte-aligned `SP`, `base < SP <= end`,
and `SP - base >= M` for an agreed positive minimum downward stack headroom.
An empty descending stack may start at `end`; the lower boundary and
insufficient headroom are rejected. The target publishes positive `M` at
CreateInvocation, and the Invocation stores it for Call-time enforcement;
there is no fixed 4 KiB floor. Require `base`, `end`, and positive `M` to each
be multiples of 16 bytes, without rounding or requiring page alignment.
Sub-page extents and non-power-of-two sizes are supported. Construction
rejects `M > end - base` before installing a capability, preserving the
destination slot and authority. Equality is valid and leaves only `SP = end`
satisfying the stack predicate. Stack-validation failures use `InvalidStack`
with the offending `u64` value in `x1` and a field-specific typed reason in
`x2`, identifying both failure and field without a packed operand index.
Base/end/minimum-headroom/SP alignment failures have distinct reasons.
The reason/value catalogue and numeric reason IDs below specify diagnostic
semantics; `InvalidStack` uses status 32, with internal predicate order below.
Admission-stage placement is specified below. This checks numeric bounds/headroom only,
not mappings, full-stack writability, or private stack-slot ownership. The operation stores
the supplied function address without construction-time mapping or executable
validation; zero
is rejected with `InvalidPointer`. The function address is not optional, and
Invocation has no Return form. No
component-interface registry or additional function-pointer validation is
required. A component loader may parse interface specifications and
prepare/distribute exports as setup policy, installing them in a `KeyTable` it
selects: the direct recipient's table or a namespace-like component's table for
API discovery and joining. Return is [`Thread.Return`](thread.md#return)
operation `0` on the explicit `CurrentReturnOnly` Thread selector at
`KeySlot::THREAD_RETURN` Slot(1), never `Named(ObjectId)`. It names no function,
AddressSpace or concrete Thread and carries no Thread-management rights.
Ordinary current-AS-table lookup, SELF, guard, incarnation, bounds and presence
checks apply; every Thread in the AS shares the sentinel, but it acts only on
the invoking Thread's own top continuation. AddressSpace provisioning
installs it before the AddressSpace can be activated; a component may still
delete its own sentinel. Because provisioning installs it, the sentinel is not
propagated through KeyTable management. Overall call lifecycle behavior remains
unspecified. Call-only Invocation distribution is a deferred decision; no
broader derivation/transfer permissions are implied.

Queued rendezvous is userspace composition over Invocation and
`Notification`/`EventCount`; it is not a kernel object kind.

## User-level visible operations

| Op | Name | Status |
|---|---|---|
| `0` | Call | `x0` Invocation capability, `x1` operation ID `0`, `x2..x7` six `u64` Call inputs; at target entry, the kernel zeroes dummy `x0`/`x1` and `x8..x30`, clears NZCV, and leaves the six real inputs unchanged in `x2..x7`, without a register shuffle or source-side dummy-zeroing requirement. Successful source resumption receives `x0 = SUCCESS (0)`, `x1 = r0`, `x2 = r1` for the two target-provided `u64` words, with kernel status separate from payload. The common native target-body return convention is experimental, with its freeze a pending maintainer decision; `Thread.Return` SVC carries the two payload words in x2/x3 and ignores x4..x7; the low-level helper exposes Result<Infallible, CapError>, and the common adapter reports Err with original payload to a non-returning libOS fault handler; userspace handler binding is provisionally link-resolved with the five-u64 extern-C non-returning ABI (status/detail1/detail2/original r0/original r1 in x0..x4); handler symbol is `vesper_thread_return_fault`. The working target-SP transport is provisionally `x9`, separate from target argument registers; the kernel consumes saved `frame.gpr[9]`. Freezing x9 is a pending maintainer decision. |

Invocation is Call-only. Opcode `0` means Call, not wrong-form Return. Every
other Invocation opcode, including `1`, is unknown and must yield
`InvalidOperation` from the active Call handler. No Return opcode or compatibility alias exists here.
Return operation, selector and validation rules belong to
[`Thread.Return`](thread.md#return).

## Invocation-depth exhaustion

A Call requiring another continuation when the depth-16 invocation stack is
full fails with **`NESTING_DEPTH = 33`** in `x0`. This is recoverable rejection
before push or context switch; the source and existing records are preserved.
It follows valid SP and target translation preparation under the admission-stage
order below. `x1` reports the current saved-continuation count, equal to the
maximum supported depth at exhaustion; `x2` is zero. The full array therefore
returns `(x0, x1, x2) = (33, 16, 0)`, not attempted depth 17. The shared
status/error encoder and lossless client decoder, inline depth-16 storage and
the non-committing Call preparation depth check are implemented: 15 saved
records admit, 16 reject with `(33, 16, 0)` through the dispatched Call path.

Return underflow remains fault delivery with no pop, not `NestingDepth`.

## Stack-validation diagnostics

`InvalidStack` uses wire status **`INVALID_STACK = 32`** in `x0`, reports the
submitted value in `x1` and a field-specific typed reason ID in `x2`, without
operand packing. IDs below are the complete `x2` values; **0 is not a valid
reason**. Predicate precedence is specified separately below; numeric IDs do
not define it. Status 32, `CapError::InvalidStack { value, reason }`, and
root-exported `InvalidStackReason` are implemented. `CapError::code()` emits
`(32, value, reason_id)`; the shared decoder checks the entire `u64` reason
without narrowing. Zero, unknown IDs and high-bit extensions become
`UnknownResponse`, preserving status and both details verbatim. The offending
value remains full-width for every known reason. `NestingDepth` status 33
(current/maximum count in `x1`, zero in `x2`) is implemented in the shared
ABI/client decoder, as is the userspace-synthesized `UnexpectedReturn`
status 34.

| ID in `x2` | Reason | Condition | Value in `x1` |
|---:|---|---|---|
| 1 | `ExtentEmpty` | `end == base` | `end` |
| 2 | `ExtentInverted` | `end < base` | `end` |
| 3 | `BaseOutsideUserRange` | Base outside the target user VA range | `base` |
| 4 | `EndOutsideUserRange` | Exclusive end exceeds the permitted user extent | `end` |
| 5 | `BaseMisaligned` | Base not 16-byte aligned | `base` |
| 6 | `EndMisaligned` | End not 16-byte aligned | `end` |
| 7 | `MinimumHeadroomZero` | `M == 0` | `M` |
| 8 | `MinimumHeadroomMisaligned` | `M` not 16-byte aligned | `M` |
| 9 | `MinimumHeadroomTooLarge` | For an ordered extent, `M > end - base` | `M` |
| 10 | `SpMisaligned` | SP not 16-byte aligned | `SP` |
| 11 | `SpOutOfRange` | `SP <= base` or `SP > end` | `SP` |
| 12 | `SpInsufficientHeadroom` | In-range SP, but `SP - base < M` | `SP` |

The exclusive end may equal the target user range's exclusive upper boundary;
it need not name an accessible byte. Empty/inverted extents have distinct
reasons; lower/upper SP-bound violations share `SpOutOfRange`. Relational
failures report submitted `end`, `M` or `SP`, not computed differences.
AArch64 supplies `ArchObjects::USER_VA_END = 1 << page_table::VA_BITS`, with
`VA_BITS = 48`: the supported low user interval is `[0, 1 << 48)`, and the
extent's exclusive end may equal `1 << 48`. This numeric ceiling does not
admit the trusted Bounce fixture's high direct-map stack as a PPC user stack.

### Stack-validation order

Stop at the first stack-validation failure in this order:

For `AddressSpace.CreateInvocation`:

1. Extent ordering: `ExtentEmpty` if `end == base`, otherwise `ExtentInverted`
   if `end < base`.
2. `BaseOutsideUserRange`, then `EndOutsideUserRange`.
3. `BaseMisaligned`, then `EndMisaligned`.
4. `MinimumHeadroomZero`.
5. `MinimumHeadroomMisaligned`.
6. `MinimumHeadroomTooLarge`.

For the numeric `InvocationStackExtent::validate_sp` helper used by the
`Invocation.Call` check:

1. `SpMisaligned`.
2. `SpOutOfRange`.
3. `SpInsufficientHeadroom`.

Check extent ordering before `end - base`, and SP bounds before `SP - base`.
An equal, misaligned extent reports `ExtentEmpty`; a simultaneously misaligned
and out-of-range SP reports `SpMisaligned`. This specifies the order among
stack predicates; the admission-stage order below places those checks in the
operation. `InvocationStackExtent::new` implements the construction predicates;
`validate_sp` implements the SP predicates, with dedicated predicate/precedence
tests. Construction checks precede installation and preserve state/authority
on rejection. Call preparation applies `validate_sp` to the SP read from the
saved frame's `x9`, never the live register, the source `SP` or neighbouring
registers.

### Admission-stage order

Capability/authority and live target identity are checked before supplied-value
diagnostics. After those succeed, supplied values are checked before remaining
destination/resource readiness. Stop at the first failed stage.

For active `AddressSpace.CreateInvocation`:

1. Validate the operation, required capabilities/types, `GRANT` on the invoked
   AddressSpace, `INSTALL` on the destination KeyTable and live target identity.
2. Reject a zero function address with `InvalidPointer`.
3. Run the ordered extent/minimum-headroom checks.
4. Check destination-slot bounds, vacancy and installability.
5. Install only after every check passes.

For `Invocation.Call` admission (implemented and dispatched):

1. Validate the operation, Invocation key, Call-only form, applicable authority
   and live target identity.
2. Run the ordered SP checks.
3. Validate/prepare the target translation context (root/ASID readiness and
   encodability).
4. Check invocation-depth capacity.
5. Push/commit only after every check passes; hardware installation follows
   after object/Access guards and incompatible locks end.

Missing authority/stale target identity takes precedence over malformed inputs.
An invalid extent plus an occupied destination reports `InvalidStack`; an
invalid SP plus an unready translation context or full invocation depth reports
`InvalidStack`. Translation-preparation failure precedes depth exhaustion once
SP is valid. This selects no new authority bits, preparation-error meanings or
lifecycle/fault semantics. The active constructor follows this order, including
full-width destination-slot representability after extent validation.

Call stages 1–4 are implemented by `api::invocation::prepare_call` (operation,
key lookup, Invocation kind, `CALL` right) and `Nucleus::prepare_call` (live
target identity, saved-`x9` SP, `prepare_translation_context`, depth). It
returns a `PreparedCall` with the source Thread identity, the source
continuation captured from the saved frame, checked target translation
metadata, entry PC, target SP, the six `x2..x7` inputs and the current depth.
It takes the nucleus by shared reference and mutates nothing on success or
rejection: the source Thread, its invocation stack, target translation state,
hardware TTBR0 and capability tables are unchanged.

Stage 5 is `Nucleus::commit_call` (with `api::invocation::call` running all
five stages). It first re-checks that the description is current (same
current Thread incarnation, still running, still in the captured source
AddressSpace, same stack depth), rejecting a stale one with
`InvalidOperation` and no mutation. It then pushes the continuation and
switches the Thread's AddressSpace to the target, so later key lookups use the
target table. It returns a `CommittedCall` carrying the scrubbed target-entry
frame (only `x2..x7` kept; `x0`/`x1`, `x8..x30` and LR zero; PC = entry;
SP = saved `x9`; source SPSR with only NZCV cleared; origin unchanged) and the
checked translation metadata. No hardware is touched by the commit.

Call is dispatched. `handle_cap_invoke` reads every input from the saved-frame
copy, and `core_invoke` routes the Invocation kind to `api::invocation::call`.
After guards and the kernel lock end, the SVC entry installs the target
translation, restores the scrubbed target-entry frame and traces
`✅ Invocation::Call()`; the Thread resumes in the target on the same core
stack. The matching [`Thread.Return`](thread.md#return) is dispatched too. The
boot fixture runs a same-Thread round trip into the Bounce AddressSpace twice
through real SVCs, checking target-entry registers, NZCV, DAIF and SP, the
target root, Bounce-only probe backing, and the source's restored x19..x30,
SP, NZCV and root. `InvocationKey::call(args, target_sp)` (over `libsyscall::ppc_call`) is the
userspace Call wrapper; `ppc_export!` generates the export entry wrapper
whose address `CreateInvocation` publishes.

## Non-payload GPR and condition-flag exposure

The [canonical PPC contract](../../nucleus_capabilities.md#selected-ppc-execution-context-directions)
selects kernel-owned migration-frame initialization. On successful Call, first
save the source continuation, including x19..x30 and source
AddressSpace/SP/PC/origin/raw SPSR, and consume the provisional target SP from
saved `frame.gpr[9]`. Then retain the six real inputs unchanged in x2..x7,
zero dummy x0/x1 and all x8..x30, clear target-entry NZCV, and install the
target execution SP and exported entry PC. Target execution-status controls
inherit the immediate source's saved admitted SPSR: execution mode, interrupt
masks and all other non-NZCV controls are unchanged. Derive status from the
saved frame, never live kernel-handler PSTATE or previous target state; EL1t
stays EL1t and EL0 stays EL0. There is no source-side dummy-zeroing requirement. Saved source x19..x30 remain kernel-private; incoming target x30
is zero because the non-returning export wrapper establishes its own
body-return linkage with a normal call.

On successful Return, capture r0/r1 from x2/x3 before rewriting the frame.
Deliver x0=SUCCESS, x1=r0 and x2=r1, zero x3..x18, and restore the top
continuation's exact source x19..x30, AddressSpace, SP, PC, exception origin
and raw SPSR, including the source's NZCV.

| State | Successful Call: target entry | Successful Return: source resumption |
|---|---|---|
| x0/x1 | Zero, dummy arguments | SUCCESS / r0 |
| x2 | First real input, unchanged | r1 |
| x3..x7 | Remaining real inputs, unchanged | Zero |
| x8..x18 | Zero, including consumed x9 | Zero |
| x19..x30 | Zero; source snapshot stays private | Exact saved source words |
| NZCV | Zero | From raw saved source SPSR |
| Other SPSR controls (mode / masks / supported bits) | Inherited from saved source SPSR | Exact raw saved source SPSR |
| Execution SP / PC | Submitted target SP / exported entry | Saved source SP / return PC |

Migration scrubbing does not apply to recoverable local Call/Return rejection;
existing preservation and shared status/detail error contracts remain unchanged.
Return still ignores submitted x4..x7 without userspace initialization or
zeroing: input consumption is distinct from clearing x3..x18 in the successfully
resumed source frame.

No extra continuation fields or runtime allocation are required. The measured
record/array sizes are 144 B/2304 B. This policy
addresses GPR/NZCV disclosure only; deliberate saved-source status inheritance
is also selected. Clearing NZCV must not zero other SPSR controls, and Return
restores the exact raw saved source SPSR even if target code changed its status.
TLS, debug state and complete architectural-state isolation remain open;
FP/SIMD trap enforcement is separate. Trusted EL1 testing is not hostile-EL0
confinement. Scrubbing and status inheritance are implemented and validated
with distinct source/target sentinels, nesting and rejection unit tests, and
the real Bounce round trip including the compiled export wrapper. The x9
transport stays provisional and the native body-result convention experimental.

## Contract details still to specify

Selected execution-context directions (canonical contract, communication
section): the kernel keeps one stack per core and a Thread's continuation
never lives on a kernel stack; each Thread carries a fixed-depth array of
kernel-owned continuation records inline in its pool entry (depth 16), holding
the source AddressSpace identity, return PC, user SP, an 8 B per-call time
stamp, source SPSR (including flags and interrupt masks), exception origin,
and the twelve source integer registers x19–x30. On admitted Call the kernel
copies these registers from the transient saved frame before migration commit;
successful Return restores the top record's exact words independently of
SUCCESS/r0/r1 in x0/x1/x2. Nested Calls keep one snapshot per immediate source;
recoverable pre-commit Call rejection preserves source registers and records.
No source spill stub is required and target code/direct Return is not trusted
to preserve the source. Storage is inline in supplied Thread pool backing,
without runtime kernel allocation. The record is 144 B and the record array
2304 B per Thread; Thread pool backing, stride and accounting derive from the
actual types. x18 is separately selected as caller-volatile and is not saved in the record;
FP/SIMD is prohibited/trapped for the current integer-only slice;
[non-payload GPR/NZCV scrubbing](#non-payload-gpr-and-condition-flag-exposure)
is implemented. Call-time depth exhaustion is reported as
`NestingDepth` (wire status 33; current/maximum count in `x1`, zero in `x2`). The target stack
extent is selected and published at
`AddressSpace.CreateInvocation`; `Call` must provide a 16-byte-aligned SP
satisfying `base < SP <= end` and `SP - base >= M` for an agreed positive
minimum downward headroom published by the target at CreateInvocation and
stored in the Invocation, using the byte count in `x7`. The extent boundaries
and positive `M` are each multiples of 16 bytes; page alignment is not required.
Construction requires `M <= end - base`, checking extent ordering before
subtracting; equality is accepted. An invalid
SP is explicitly rejected before the invocation-stack push or context switch,
preserving the source context and invocation stack, with `InvalidStack` carrying
the offending `u64` value in `x1` and a field-specific typed reason in `x2`.
Its wire status is `INVALID_STACK = 32`. The kernel checks the declared
range only, not page mappings or full-stack writability. The SP handoff is
separate from target function arguments: target-entry `x0` and `x1` are two
leading dummy arguments ignored by the target; the six real `u64` inputs stay
unchanged in `x2..x7`. Return is
`Thread.Return` operation `0` on the `CurrentReturnOnly` Thread sentinel,
popping only the invoking Thread's own record and restoring source AddressSpace,
SP, and PC with return values in designated registers. A Return that cannot
complete its protocol (depth-zero underflow; a saved source AddressSpace that
is no longer live) is a fault to the invoking Thread's fault handler, with no
pop. Return on a valid named Thread entry is a recoverable `InvalidOperation`;
`CurrentReturnOnly` rejects management operations and `object_id` extraction.
Opcode zero on an ordinary Invocation is Call, not wrong-form Return. The
blocked-wait continuation is implemented in Thread-resident `SavedContext`
storage, so no kernel-stack frame carries a continuation for waits either. The PPC array
is implemented inline in each Thread. The record's stamp is taken at Call;
attributing the elapsed time to the source Thread's own DCB on Return is not
implemented yet, and broader hierarchical attribution is deferred.
Each AddressSpace names exactly one keytable shared by its Threads. `Call`
uses the target AddressSpace's keytable, and the continuation's source
AddressSpace identity selects the keytable restored by `Return`. AddressSpace
provisioning and ordinary caller lookup implement the immutable carved-table
association. Checked root/ASID switching uses independently provisioned
source/Bounce roots with ASIDs 1 and 2 and current-AS keytable resolution, both
for the two-Thread wait/resume fixture and for same-Thread PPC Call/Return.
This is EL1t execution on SP_EL0 with a shared high SP_EL1 trap stack, not
protected EL0 confinement.

Still to specify:

- Call-only Invocation distribution/derivation, rights attenuation, and source
  badge semantics.

- TLS, debug state and complete architectural-state isolation beyond the
  selected GPR/NZCV exposure policy and saved-source status inheritance.
- Nested/concurrent-call rules beyond the depth bound.
- The `Return` fault-handler binding, vector, and any resume-with-edited-state
  semantics (D1's open fault-handling decision; NOVA's per-vector exception
  portal is the strongest found precedent).
- Pointer/shared-memory rules, and whether capability transfer is supported.
- Return, fault, cancellation, and Thread-teardown outcomes.

The exported procedure takes two leading dummy arguments, followed by the
six real `u64` inputs. Call keeps the real inputs in `x2..x7`, rather than
shifting them into `x0..x5`. The integer result payload is two `u64`
target-provided words, separate from kernel transport status. Either word may
carry application-level status under the component interface; the kernel does
not interpret it as transport status. Successful Return resumes the source's
Call with `x0 = SUCCESS (0)`, `x1 = r0` and `x2 = r1`. The kernel supplies
transport status independently of payload. Recoverable pre-commit Call failures
retain the shared status/detail layout.

The raw-SVC integer compiler declarations are conservative and explicit:

| Wrapper | Input/result operands | Inputs with discarded outputs | Discarded clobbers |
|---|---|---|---|
| Call | x0..x2 (`inlateout`) | x3..x7 arguments, provisional x9 target SP (`inlateout => _`) | x8, x10..x18 (`lateout => _`) |
| Return | x0..x2 (`inlateout`) | x3 second payload (`inlateout => _`) | x4..x18 (`lateout => _`) |

Discarded outputs do not add result words or promise register preservation.
Ignored Return x4..x7 need not be initialized/zeroed. Call's x9 is supplied in
the SVC asm block and read from saved `frame.gpr[9]`, not live x9 after Rust;
it remains provisional until the maintainer freezes it. Rely on kernel-owned
x19..x30 preservation; add no x3..x17 continuation fields. Keep compiler
memory/flags effects conservative, without nomem/readonly/pure/preserves_flags;
initially omit nostack pending switched-stack validation. These memory effects
are not alone a hardware ordering guarantee. The fallible Return SVC is not
noreturn, even though the enclosing export wrapper returns `!`. Out-of-line
C function ABI effects differ from raw SVC; blanket clobber_abi(C) is not this
integer-only contract. x18 is ordinary caller-volatile scratch, a discarded
lateout clobber in both wrappers, without kernel preservation/record growth,
compiler reservation or a reserved libOS platform/TLS role. Components cannot
rely on it surviving Call. The kernel zeroes x18 at target entry and successful
source resumption under the selected GPR/NZCV policy; other architectural-state
isolation remains open. FP/SIMD use is prohibited/trapped for this slice as
specified below.
Both wrappers are implemented and exercised through real PPC. Optimized
call sites keep nothing live in x3..x18 across the SVC, and compiler-allocated
live values survive a target that garbages x4..x30 and NZCV.

The current slice is integer-only: kernel, component bodies, wrappers and
linked runtime code retain the soft-float/no-FP-NEON build contract. Explicitly
configure and validate effective hardware FP/SIMD traps before component
execution, covering trusted EL1t Bounce and eventual EL0; compiler settings
alone do not enforce this. Attempted use is an execution fault, not ordinary
Invocation rejection or permission to automatically enable FP. D1 kernel-origin
fault binding/delivery/resumption remains separate; this does not select the
userspace Return-error handler as a kernel-fault mechanism. Add no vector,
FPCR/FPSR or other FP/SIMD saved state. Support is deferred until initialization,
PPC/wait/scheduling preservation and isolation are designed with accounted
backing. Trusted EL1 code must obey and not modify privileged trap controls;
this fixture cannot prove hostile confinement. Trap enforcement and negative
fault tests remain unimplemented.

Common export setup supplies a per-procedure non-returning export wrapper's
address to `AddressSpace.CreateInvocation`. The wrapper has an `extern "C"`
eight-u64-argument entry returning `!`: two dummy arguments followed by six
real inputs. Call enters it by exception return on the submitted target stack,
not BL; it does not use incoming x30 as a source-return address. It calls its
linked body normally with the same positional arguments, real inputs in
x2..x7 and kernel-initialized dummy x0/x1, establishing target-local body-return
linkage. No source-side or additional adapter dummy-zeroing is required. It then
follows the selected result spill/key lookup/fallible Return/fault-handler
sequence, with no normal wrapper return, automatic retry
or body rerun. No kernel-installed target return trampoline, new Invocation
metadata or constructor operands are needed. This is a common userspace
convention, not kernel signature/executable-entry validation; custom entries
and direct Return remain possible. `ppc_export!` generates this wrapper; it is
validated through real same-Thread Calls into Bounce.

The common native target-body return convention is experimental: the userspace
export wrapper calls an `extern "C"` body returning a `#[repr(C)]` struct with
two ordered `u64` fields. AAPCS64 returns the first word in x0 and the second
in x1, without a hidden result-buffer pointer. The adapter moves native x0/x1
results into x2/x3 before loading Return key/op in x0/x1. Return's x0 is the
packed `CurrentReturnOnly` Thread key resolved from target-table Slot(1)
(`KeySlot::THREAD_RETURN`), not bare slot index 1; x1 is `Thread.Return`
operation `0`. The kernel consumes x2/x3 through the common operation-operand
transport, then delivers the payload to source x1/x2 with independent SUCCESS
in x0.
Return ignores x4..x7: their values do not affect admission, pop, switch or
result delivery, carry no extra payload/capabilities, and need not be cleared.
No reserved-zero check applies. Key/form, depth and saved-source-liveness
validation is unchanged. This input rule is separate from selected source
preservation, wrapper clobbers and kernel zeroing of x3..x18 on successful source
resumption; the adapter still need not initialize or clear ignored operands. The experimental native body-return
convention is userspace policy, not kernel signature enforcement; direct Return
remains possible. A compiled body's results reach the source through the
adapter and real Call/Return. Freezing the convention is a pending maintainer
decision.

The low-level Return helper exposes `Result<core::convert::Infallible, CapError>`.
Successful Return abandons target execution, so no Ok value returns locally.
An ordinary pre-commit rejection, such as an empty/stale key or Return on a
named Thread entry, returns Err through the shared error decoder with
diagnostics and no pop/migration.
The helper does not automatically trap or reclassify ordinary errors as faults;
libOS chooses diagnosis/recovery. Underflow and retired saved-source retain
fault delivery with no pop, not ordinary helper Err. Never fabricate an
Infallible/Ok value.

An unexpected local status-zero response from the Return helper becomes
`Err(CapError::UnexpectedReturn { word1, word2 })`, retaining local x1/x2
verbatim. The shared diagnostic status is `UNEXPECTED_RETURN = 34`;
`CapError::code()` emits `(34, word1, word2)`. This is a userspace-synthesized
protocol/invariant error, not an ordinary pre-commit rejection or proof of
unchanged continuation state/safe retry. Correct successful Return never
returns locally. Do not fabricate Infallible/Ok, use unchecked-unreachable,
encode zero as UnknownResponse or automatically trap/retry. The ordinary
syscall decoder still recognizes zero as success for other operations.
The adapter passes this error plus the original body `(r0,r1)` to
`vesper_thread_return_fault`; anomaly diagnostics do not replace that payload.
The shared status, helper and adapter are implemented; host tests cover the
synthetic error and the boot fixture validates the handler handoff.

On Err, the common export adapter transfers control to a libOS-supplied,
non-returning fault handler with decoded error diagnostics and original
(r0,r1). After body return and before key lookup/helper calls, the adapter
saves both ordered words in a 16-byte adapter-owned area of its target
execution-stack frame, with SP 16-byte aligned. It loads Return payload x2/x3
and error-handoff originals x3/x4 from this pair, independently of register
clobbers and error status/details. Export setup budgets the payload area and
other adapter-frame overhead in the published stack requirement/headroom;
this is not proof of mappings/writability or sufficient body/handler stack.
No heap/kernel allocation, invocation-record fields or dedicated x19/x20
retention requirement is introduced. Source preservation and wrapper clobbers
follow their separate selected contracts; stack-pool reuse remains open. The spill is
implemented (`export::complete_with`). Whether the adapter and body fit in the
published headroom is the component's concern; neither the kernel nor
Kickstart checks it, and a component wanting overflow protection places guard
pages around its stack extent. The handler must
not return normally to the already-completed body/adapter. LibOS may diagnose,
repair and explicitly retry Return, or terminate under its own policy; the
adapter does not automatically retry or rerun the body. This is userspace
reporting of ordinary rejection, not a new kernel fault syscall or error
reclassification; the invocation records remain intact on ordinary rejection.
The synthetic UnexpectedReturn error does not carry that state guarantee.
No separate dispatcher
from kernel-origin faults is mandated, and D1 kernel delivery/binding/resumption
remains open.

The working userspace handler binding is provisionally link-resolved: the
component image supplies the exported libOS entry point
`vesper_thread_return_fault` when linking its common export adapter, which
calls it directly in userspace. The entry point may dispatch
internally under libOS policy. No nullable runtime callback, runtime handler
registration or kernel registration operation is selected for this path. The
image must supply the handler without a silent returning/default-success stub.
This binding is for the current trial, not a frozen final model or D1
kernel-origin fault binding. Its calling convention is an ordinary userspace
`extern "C"` call taking five `u64` words in x0..x4: error status, error detail 1,
error detail 2, original r0, original r1. The error triple comes from
`CapError::code()`; no Rust enum layout or pointer-based diagnostic record
crosses the boundary. The handler returns `!`. This is not SVC or a new kernel
fault operation. Original-payload retention uses the selected target-stack
spill and entry/linkage uses the selected non-returning wrapper.
FP/SIMD is prohibited/trapped for this slice; non-payload GPR/NZCV scrubbing is
selected, as is saved-source target-entry SPSR mode/mask/non-NZCV control
inheritance; TLS/debug/other architectural-state isolation remains open.
Scrubbing, status inheritance and PPC result transport are implemented and
validated through real Call/Return.

## Kernel-level storage and numeric validation

`kernel/nucleus/src/objects/invocation.rs` defines immutable
`InvocationStackExtent::new(base, end, minimum_headroom, user_end_exclusive)`.
Private `u64` fields hold the validated base, exclusive end and minimum;
`base()`, `end()` and `minimum_headroom()` expose them without mutation.
`validate_sp(sp)` checks alignment, `base < sp <= end` and `sp - base >= M`,
in that order. Neither method walks mappings, allocates storage, proves
writability/exclusivity, or admits a Call.

The extent is 24 B/alignment 8. `InvocationPayload` is 40 B/alignment 8:
nonzero entry at offset 0, target identity fields at offsets 8/10/12, and
validated extent at offset 16. `KeyPayload` is 40 B, making `KeyEntry` 64 B
with alignment 32 (64 B slot stride); the 32 B table header is unchanged.
`KeyEntry::new_invocation` requires the validated extent alongside the target
identity and mandatory `NonZero<u64>` entry. `invocation_stack_extent` reads
it with a kind check. [KeyTable storage](key_table.md#kernel-level-implementation-details)
and Untyped accounting use the actual entry size; a 256-entry carve is
17,440 B including its header and separate incarnation counters.

## Implementation status

The shared `CoreType` catalogue recognizes Invocation at ID 7.
`AddressSpace.CreateInvocation` construction is active, with a checked target
AddressSpace identity and CALL-only authority. `InvocationPayload` stores a
mandatory `NonZero<u64>` function address; zero is rejected as `InvalidPointer`.
`InvocationOp` contains only Call `0`, and `invocation_target` returns the
checked AddressSpace identity and nonzero entry address. Return authority is
represented separately by `ThreadSelector::CurrentReturnOnly`, installed at
`KeySlot::THREAD_RETURN` Slot(1) of every AddressSpace's table by
`KeyTable::bind_address_space` during provisioning.
`AddressSpaceKey::create_invocation(function_address, destination,
destination_slot, stack_base, stack_end, minimum_headroom)` uses
`protected_call6` and forwards all six operands unchanged. The active kernel
validates authority/live target, nonzero entry and ordered extent/minimum
predicates before destination admission, then installs the validated extent
inline. Status 32 encoding/decoding and the numeric SP helper are implemented;
dedicated ABI, wrapper, predicate, storage and construction regression tests
exist. Test-run results are recorded in the implementation plan, not inferred
from the presence of those tests.
The kernel Call handler, same-Thread PPC migration, invocation stack and
`Thread.Return` dispatch are implemented and exercised through real SVCs in the
boot fixture, including through the userspace `InvocationKey::call` wrapper
and the `ThreadReturnKey::return_from_invocation` helper, and through a
compiled `ppc_export!` export including its fault-handler handoff.
Kernel-origin fault delivery is not designed:
Return faults halt the kernel under the interim policy.
