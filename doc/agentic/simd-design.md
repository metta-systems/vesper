# FP/SIMD design

The numbered sections are the model (SIMD-18), the maintainer's decisions
(SIMD-1 to SIMD-13) and the invariants the design keeps (SIMD-14 to SIMD-17);
the sections between them describe the mechanism and argue its correctness. No FP/SIMD decision is open.
Open tasks and future work are in
[simd-implementation-plan.md](simd-implementation-plan.md), finished ones in
[simd-completed.md](simd-completed.md).

FP/SIMD is opt-in, chosen per component, and the plain eager design is
implemented and checked in QEMU and on the Pi 4. On the nucleus side: the flag,
`FPEN` set by every translation install, eager save/restore through the
nucleus's only FP routines (`kernel/nucleus/src/objects/arch/fp_unit.rs`,
allow-listed by `_audit-fp-simd`), the counters, and the entry guard behind
`fp_entry_trap`. They are exercised by the nucleus tests (transition logic
against a fake unit, the routines against the hardware registers) and by
`fp-trap-test`, whose FP AddressSpace executes FP at EL0 (`_test-fp-trap`,
`_test-fp-entry-trap`). On the build side: the hard-float target, the
Justfile's `fp_components`, `tools/fp-scan` deciding `ComponentImage::fp_simd`
in image-build and running `_audit-fp-simd`, `Builder::component` taking the
flag from the image, and the `libsyscall` FP clobbers. `fp-test` exercises it
end to end in QEMU (`_test-fp`): two FP components preempted with sentinels in
every V register, FPCR and FPSR, a Call from one into the other, a Call into an
integer-only component whose FP instruction traps, and a handled fault. It
passes on the Pi 4 too (`just boot-fp-test`).

## SIMD-18: FP/SIMD per AddressSpace

The model in one place. FP/SIMD is opt-in, chosen per component (SIMD-1). The
right to execute FP belongs to the AddressSpace: a flag fixed at its creation,
from which every translation install sets `CPACR_EL1.FPEN`, so an integer-only
AddressSpace traps FP at EL0 and EL1 (SIMD-2). The register state belongs to
the Thread and is switched eagerly, skipping integer-only frames (SIMD-3). A
Call enters its target zeroed and Return restores the preserved registers
(SIMD-4); a fault hands the handler a fault frame (SIMD-9) in its pool's fault
area (SIMD-13). Whatever the path, the kernel keeps the invariants SIMD-14 to
SIMD-17: FP only in an FP frame, the register file written in full before an FP
frame is entered, no kernel FP outside its routines, and a frame's state saved
before anything overwrites it. The nucleus counts FP transitions for the
schedulers (SIMD-11), and debug kernels add a hardware check of the kernel's
abstinence (SIMD-12).

## SIMD-1: FP/SIMD is opt-in, per component

2026-11-18. FP/SIMD is opt-in, chosen per component. The kernel and
integer-only components still build soft-float; FP components build for a
hard-float target.

## SIMD-2: the right to use FP comes from the AddressSpace

2026-11-19. An AddressSpace carries an FP flag, set at creation (bit 8 of
`Untyped.Retype`'s `x3` for an AddressSpace; the bit is reserved zero for every
other kind) and never changed. Every installation of a translation context also
sets `CPACR_EL1.FPEN`. An FP AddressSpace runs untrapped; any other traps
FP/SIMD at EL0 and EL1, and a use there is an execution fault, as before. SVE
stays trapped everywhere.

## SIMD-3: plain eager save and restore first

2026-11-19. The register state is per Thread: V0–V31, FPCR and FPSR (528 B),
held in `Thread.fp` while the Thread is off the CPU. A Thread starts with it
zero. The first implementation switches eagerly and skips integer-only frames:
leaving an FP frame saves what that frame needs back, and entering one first
writes the whole register file, from saved state or zeros. Transitions between
integer-only frames cost nothing. Usage tracking comes later (SIMD-10).

## SIMD-4: a Call enters zeroed, Return restores the preserved registers

2026-11-19. A Call enters the target with the register file zeroed, and Return
restores `q8..q15`, FPCR and FPSR. Call and Return follow the
[register table](capabilities-contract.md#call-invocationcall-op-0); a fault
follows [fault delivery](capabilities-contract.md#fault-delivery).

## SIMD-5: a Justfile list chooses the hard-float crates

2026-11-19. A list in the Justfile, `fp_components`, chooses which crates build
hard-float. A marker in the crate itself is future work (N-SIMD-1).

## SIMD-6: the runtime FP flag comes from scanning the ELF

2026-11-19. The runtime FP flag is derived by scanning the ELF. The Rust
checker that does it also replaces the disassembly in `_audit-fp-simd`. A
component with FP hits that is not in `fp_components` fails the build.

## SIMD-7: a fault handler can inspect the FP state

2026-11-18. A fault handler can inspect the faulting frame's FP state.

## SIMD-8: the Thread holds everything inline

2026-11-18. The Thread holds all FP state inline, at about 6 KiB, and shrinks
later in the measured optimisation pass.

## SIMD-9: the handler gets a fault frame

2026-11-19. The handler gets the faulting state as a fault frame; where the
frame lives is SIMD-13.
- The frame includes the general registers.
- The handler may write the frame back.
- The kernel keeps only the general registers, and the FP state lives only in
  the frame. See "The fault frame".

## SIMD-10: usage tracking with decay and owner tracking, later

2026-11-19. After the eager baseline has been measured, a decaying usage flag
together with owner tracking replaces plain eager switching, and the first-use
trap arrives with it. FP statistics are counted per Thread from the start.

## SIMD-11: FP statistics in the scheduler-shared records

2026-11-19. The nucleus counts FP transitions per Thread and publishes them in
the Thread's scheduler record, with totals in the root Scheduler's record (see
the record layouts in
[`scheduling-design.md`](scheduling-design.md#scheduler-shared-records)).
They are scheduling information, which the root scheduler can read and act on,
so the FP policy can become adaptive later.

## SIMD-12: the entry guard runs in debug kernels

2026-11-21. A nucleus feature `fp_entry_trap` turns FP off at every kernel
entry from an FP frame and back on before the `eret` into one, so a stray
kernel FP instruction traps at EL1. `debug_kernel` turns it on; release kernels
run without it ("we expect the privileged nucleus to be small and simple enough
to never use fp/simd, so the overhead is actually a waste there"). Measured on
the Pi 4 at 0.3 % of a Signal and 1.3 to 2.4 % of a Call from an FP frame
(`doc/simd-performance.md`). In release kernels SIMD-16 rests on the build and
the audit, which therefore must not pass while checking nothing: see "The
model".

## SIMD-13: the fault frame goes into a registered fault area

2026-11-21. The kernel never touches pool stacks. The fault frame goes into the
*fault area* of the handler's pool, a RAM Frame registered at
`CreateStackPool`, and the kernel writes it through its own mapping of that
Frame. See "The fault frame".

## Scope

Execution in an AddressSpace without the FP flag is integer-only (SIMD-2). It is
enforced at four points:

- The kernel and every component build soft-float (`targets/aarch64-metta-none-eabi.json`,
  `-neon,-fp-armv8`).
- `CPACR_EL1.FPEN` traps FP/SIMD at EL0 and EL1 (`configure_fp_simd_traps`,
  `kernel/kickstart/src/el_switch.rs`).
- `just _test-fp-trap` checks that the trap fires.
- `just _audit-fp-simd` rejects any image that contains an FP/SIMD instruction.

This design lets chosen components use FP and NEON. Their register state is kept
per Thread, and confidentiality across protection domains still holds. Components
that are not chosen keep today's behaviour and cost.

Raspberry Pi 3, 4 and 5 (Cortex-A53, A72, A76) have FP and NEON and no SVE. SVE,
SME and kernel use of SIMD are out of scope.

## Why the AddressSpace, not only the Thread

The register state belongs to the Thread, like its general registers. What is tied to
the AddressSpace is the permission to use FP and the handling where a Thread crosses
from one AddressSpace into another:

- **The code belongs to the AddressSpace.** A Thread runs whatever its PC reaches
  through the current AddressSpace's mappings. Inside a PPC Call, that is the callee
  component's binary. Whether that binary contains FP instructions depends on how the
  component was built. An FP instruction in a component that was not built for FP is
  a bug or hostile code, and trapping it is the confinement the contract asks for.
- **A Call changes AddressSpace without changing Thread.** A per-Thread "FP enabled"
  flag would leave FP enabled inside an integer-only callee, which could then read the
  caller's V registers. A per-Thread flag also cannot let an integer-only client call
  an FP service: either the call faults, or every Thread needs the flag.
- **General registers already follow this pattern.** Call enters the target with
  `x0..x30` zeroed apart from the arguments, and Return restores `x19..x30` from the
  call record (`doc/object_types/core/invocation.md`, "Register state"). The FP rules
  below extend that to the V registers.

## The model

1. **The AddressSpace gets an FP flag.** `AArch64AddressSpace.fp_simd: bool`
   (`kernel/nucleus/src/objects/arch/address_space.rs`) is set when the AddressSpace
   is created and never changes. It is carried in bit 8 of Retype's `x3` word, the way
   Frame and Untyped carry their memory-attribute ceiling in bits 15:8
   (`kernel/nucleus/src/api/untyped.rs`). It is passed to `new_address_space`. No new
   kind and no new operation are added.
2. **FP access follows the AddressSpace being entered.** Each translation-root install
   also sets `CPACR_EL1.FPEN`:
   - an FP AddressSpace gets `0b11`, no trap;
   - an integer-only one gets trap-EL0-and-EL1, as today.

   An FP instruction in an integer-only AddressSpace stays a `CpuException` fault and is
   delivered as now.
3. **The register file is written in full on entry.** Before entering an FP frame, the
   kernel writes all of V0–V31, FPCR and FPSR, either by restoring saved state or by
   zeroing. Leaving an FP frame saves only what that frame needs back. Nothing is
   scrubbed on the way out, and transitions between integer-only frames cost nothing.
4. **The kernel stays soft-float.** Its only FP code is the save, load and zero routines,
   in `global_asm!` under `.arch_extension simd`. Each sets `FPEN` to allow EL1 for its
   duration, and entering the target AddressSpace writes the final `FPEN` value.

   AArch64 has no `FPEN` setting that allows EL0 while trapping EL1, so a kernel entry
   from an FP frame starts with FP enabled. A kernel FP instruction at that point would
   silently corrupt the frame's live registers.

   The entry guard (SIMD-12) turns FP off at every entry from an FP frame,
   and back on before the `eret` into one, so the kernel runs trapped outside its own
   routines. It is the nucleus feature `fp_entry_trap`, which `debug_kernel` turns on;
   release kernels run without it, since the nucleus is expected never to use FP and
   the guard would cost 1.3 to 2.4 % of every Call from an FP frame
   (`doc/simd-performance.md`). The debug kernels, which the test kernels are, are
   therefore the run-time check of the audit.

   `_audit-fp-simd` stays mandatory in `just lint`, and must never pass while checking
   nothing, as it did once when an `nm` output format mismatch kept it from reading the
   nucleus at all:
   - it finds every soft-float image the build produced by itself, the release kernels
     included, instead of reading a list kept by hand;
   - an image with no code to scan fails;
   - each function the allow-list names, the nucleus's FP routines, must hold FP
     instructions, so every image that links the nucleus shows that the scan sees them;
   - `fp-probe`, which executes FP on purpose, must show FP instructions.

Terms used below:

| Term | Meaning |
|---|---|
| Frame | One level of a Thread's execution in one AddressSpace. The base level is one frame, and each Call adds one. |
| FP frame | A frame whose AddressSpace allows FP. |
| Register file | V0–V31, FPCR and FPSR. |

## Where FP state lives

All storage is inline; the kernel allocates nothing.

| Storage | Holds | Written when | Size |
|---|---|---|---|
| `Thread.fp: FpState` | The current frame's FP state while the Thread is off the CPU | At a Thread switch away from an FP frame | 528 B: 32 × 16, plus FPCR and FPSR, 16-aligned |
| `InvocationContinuation.source_fp: FpPreserved`, and an "FP preserved" bit | The caller's `q8..q15`, FPCR and FPSR | At a Call or fault delivery from an FP frame | 136 B and 1 bit |

The kernel keeps all 128 bits of `q8..q15`. AAPCS64 only requires a callee to preserve
the low halves, `d8..d15`, but Rust's `asm!` clobber model treats `v8..v15` as
preserved in full unless they are named. Keeping all 128 bits means `libsyscall`'s
`ppc_call` needs no FP spills of its own.

FPCR holds the rounding mode and other controls, which AAPCS64 says a call preserves.
FPSR holds the cumulative exception flags. It is preserved too (SIMD-4), so a caller
that tests its flags across a call keeps them. The record is 128 + 4 + 4 bytes, the
136 B in the table. Every value is restored from the caller's own record, so nothing
from the callee leaks back.

A fault interrupts the frame between two arbitrary instructions, so every V register
is live, and its full FP state has to be kept. Under SIMD-9 the kernel keeps only the
general registers, in `ThreadFault.frame`, as it does today. The FP state's only copy is
the fault frame in the handler pool's fault area, described below. There is no
`ThreadFault.fp`.

### The fault frame

The handler receives the faulting state as a fault frame (SIMD-9), in its pool's fault
area (SIMD-13). The kernel never touches pool stacks.

- **Where it lives.** A pool that serves a fault handler is made with a *fault area*:
  a RAM Frame (`READ|WRITE`), mapped in the pool's AddressSpace, passed in
  `CreateStackPool`'s `x5`. It holds one 1 KiB slot per stack (`FAULT_SLOT_BYTES`), so a
  4 KiB Frame serves up to 4 stacks. The kernel writes the frame into the slot of the
  stack the forced Call takes, slot `i` for stack `i`.
  - The stack and its slot are leased to this one call, survive Thread switches while
    the handler runs, and are released at the handler's Return.
  - The handler starts with SP at the top of its stack, like any Call.
  - `x0` holds the slot's address: the Frame's mapped address, recorded when the pool
    is made, plus the slot offset. A fault delivery is a plain Call, so `x0` would
    otherwise be zero. `FaultInfo` stays in `x2..x7`.
  - This follows `Scheduler.ShareRegion`: the Frame is checked once, at registration,
    and stays valid because the nucleus does not reclaim Frame memory.
- **What it holds** (`libobject::fault::FaultFrame`, 832 B, 16-aligned):
  - a header with size, version and flags, including whether an FP part is present;
  - the faulting general registers (SIMD-9): `x0..x30`, SP, PC, `pstate`, `TPIDR_EL0`;
  - for an FP frame, the V registers, FPCR and FPSR.
- **How it is written.**
  - At delivery, `FpUnit::save` puts the faulting FP state into a kernel-stack copy of
    the frame, before the forced Call zeroes the registers for an FP handler.
  - The nucleus writes the slot through its own mapping of the Frame. It does not walk
    the handler's translation tables, and the handler AddressSpace need not be the
    active one. The page is a capability-named Frame, not a page found by a walk, so a
    later multicore kernel has no unmap race between finding the page and writing it.
  - The write happens before the forced Call commits. A pool without a fault area
    makes the fault unhandled with nothing to undo, like a full invocation stack. The
    Thread becomes `Faulted` with its general registers, and its FP state is dropped.
  - A handler that unmaps the Frame later finds nothing at `x0`; the kernel's write
    still lands in the Frame.
  - The software walk that the frame's first placement used is kept, unused, in
    `kernel/nucleus/src/user_memory.rs` for a DebugConsole that reads its caller's
    buffer by address.
- **Write-back (SIMD-9).** At a Retry or Skip Return, the kernel resumes from the frame,
  which the handler may have changed. This needs no new trust: the handler can change
  only the state of the Thread it handles, which it is already trusted with. The kernel
  still checks `pstate`, because that check protects the kernel, not the Thread.
  - The kernel copies the slot once onto its own stack, through the same mapping, and
    validates that copy. It never validates in place, since another Thread of the
    handler's AddressSpace could change the frame in between.
  - The header must be unchanged, and `pstate` may differ from the faulting one only in
    the condition flags (NZCV), so the Thread resumes at the same EL with the same masks.
  - The FP part goes from the copy into the V registers with `FpUnit::load`.
  - An invalid frame terminates the Thread. Its `Faulted` state is the kernel's copy of
    the general registers, `ThreadFault.frame`, as today. The original state is never
    silently resumed.
  - Terminate leaves the frame in the slot for inspection.
- **Which copy counts (SIMD-9).**
  - The kernel keeps the general registers in `ThreadFault.frame`. That copy is the
    fallback for `Faulted` and the unhandled-fault path.
  - The FP state lives only in the frame.
  - A valid frame is always what a Retry or Skip resumes, both its general registers
    and its FP state.
- **Trust.** The handler is the faulting AddressSpace's own designated handler, and it
  can already terminate or skip the Thread. Changing that Thread's registers adds no new
  trust. The `pstate` check protects the kernel, not the Thread.
- **Confidentiality.** The frame shows the faulting frame's values to the handler's
  domain. That is the deliberate, authorised exception to "a frame sees only its own
  values or zeros". The handler is the domain meant to see them.

**Size.** The Thread grows from about 3.3 KiB to about 6 KiB:

| Addition | Size |
|---|---|
| `Thread.fp` | 528 B |
| 16 call records × 136 B | 2176 B |
| `Thread.fp_stats`, plus `fp_budget` from P-SIMD-5 | 64 B, plus 1 B |

- The carve size follows the type (`carved_layout::<Thread>()`), so Untyped
  accounting adjusts by itself.
- The 176 B continuation assert in `kernel/nucleus/src/objects/thread.rs` must be
  updated.
- The size is accepted (SIMD-8). Shrinking the Thread belongs to the measured
  optimisation pass that comes after correctness.

## Transitions

All five TTBR0 writes are in `kernel/nucleus/src/main.rs`. The supporting logic is in
`kernel/nucleus/src/objects/resume.rs`, `invocation.rs` and `fault.rs`.

| Transition | Site | Leaving an FP frame | Entering an FP frame | Entering integer-only |
|---|---|---|---|---|
| Thread switch: preemption, block, dispatch, idle wake, exit | `install_selected`; outgoing state captured in `stop_current`, next to the general registers | Save everything to the outgoing `Thread.fp` | Load the incoming `Thread.fp` | Trap |
| Call and CallWithKey | `commit_call` | Save `q8..q15`, FPCR and FPSR to the pushed record, and set its bit | Zero everything; FPCR is 0 | Trap |
| Fault delivery, a forced Call | `deliver_fault` | Save everything into the fault frame, written to the handler pool's fault area before the Call commits; the record as for a Call | Zero | Trap |
| Return | `commit_return` | Nothing | With the record's bit set: load `q8..q15`, FPCR and FPSR and zero the rest. A fault Return copies the fault frame once, validates the copy, and loads its FP part in full | Trap |
| `AddressSpace.Activate` | — | Same AddressSpace, no change | | |
| Exit, retire, terminate | `exit_current_thread`, `Thread.Retire`, `park_faulted` | Nothing: the state is dead, and the next FP entry overwrites it | | |

How some cases fall out of this table:

- A Thread blocked inside a callee, `ExecutionContext::Parked`, is an ordinary Thread
  switch.
- A new Thread's `Thread.fp` is zero from the carve.
- The boot Thread and the `EL1t` fixture Threads stay integer-only.

The kernel touches no FP register between exception entry and `eret`. So the outgoing
values are still in hardware whenever the Rust handler runs. A save may happen anywhere
before the incoming load, and `vectors.S` does not change.

Saving or loading the full state is 16 `stp` or `ldp` pairs plus two system-register
accesses, about 1 KiB of memory traffic counting both directions. That is on the order
of 100 to 200 cycles on an A72, an estimate that has not been measured. It happens only
on transitions that involve an FP frame.

### The hardware interface

A trait sits next to `install_translation_context` in
`kernel/nucleus/src/objects/arch_objects.rs`. Its methods:

- `FpUnit::save` and `load` move the whole register file;
- `zero` clears it;
- `save_preserved` stores `q8..q15`, FPCR and FPSR, and `load_preserved` writes them back and
  zeroes the rest.

The object layer decides what to do and calls the trait. The aarch64 implementation,
`HardwareFpUnit` in `kernel/nucleus/src/objects/arch/fp_unit.rs`, is six naked
`fp_unit_*` functions under `.arch_extension fp, simd`. Each opens `FPEN` around itself
and puts back what it found, since the object layer calls them before the target is
installed. The nucleus tests use a fake register file for the transition logic, and test
the hardware routines against the real registers in QEMU.

Access follows installation: `install_translation_context` takes the AddressSpace's FP
flag, carried in `PreparedTranslationContext`, and sets `FPEN` together with
`TTBR0_EL1`, under the same `isb`. All five install sites in `kernel/nucleus/src/main.rs`
go through it, so no site can forget the trap.

## FP statistics

The kernel counts from the start (SIMD-10). The eager baseline is therefore measured
before usage tracking changes anything. Every counter is a `u64` that wraps, and the
kernel only increments it.

**Per Thread.** The counters live in `Thread.fp_stats` and are published in the
Thread's scheduler-shared record (SIMD-11). `ThreadRecord`
(`libs/object/src/scheduler_record.rs`) has `reserved: [AtomicU64; 8]`. That is
exactly these eight counters, so the record's 128 B size and its stride stay as they
are. The fields get names in the layout, and `LAYOUT_VERSION` is not bumped, because
there is no earlier version to stay compatible with. The nucleus copies the counters
into the record when the Thread is switched away from. That is already when it
publishes `run_time_ns`, under the same sequence protocol. A Thread with no assigned
record keeps them in `Thread.fp_stats` only.

| Counter | Counts |
|---|---|
| `fp_entries` | Entries into an FP frame |
| `fp_saves` | Full saves, to `Thread.fp` or into a fault frame |
| `fp_loads` | Full loads of a saved state |
| `fp_zeroes` | Entries into an FP frame with the register file zeroed (Call and first start) |
| `preserved_saves` | `q8..q15`, FPCR and FPSR saved to a call record |
| `first_use_traps` | With usage tracking: first-use traps taken |
| `decays` | With usage tracking: times the usage flag decayed |
| `owner_hits` | With usage tracking: saves and loads skipped because the hardware still held this Thread's frame |

**Totals.** These live in the nucleus next to `current_thread`, and are published in
the root Scheduler's `SchedulerRecord`, which also has eight reserved words:

- Thread switches;
- Thread switches that involved an FP frame on either side;
- with the entry guard, the `FPEN` toggles at kernel entry and exit.

Sums of the per-Thread counters need no totals of their own, because a scheduler
can add up its Threads' records.

**Why scheduler records.** The counters are scheduling information. The root
scheduler, or any scheduler that holds the record, can read them and act on them. This
leaves room for a later adaptive FP policy that makes the decisions in userspace, for
example placing FP-heavy Threads differently or choosing the decay budget. Moving the
budget out of the kernel would itself be a separate decision.

The cost is one increment per FP transition, which is small next to a 528 B save, plus
the copy into the record at the switch away.

What the counters answer:

- whether Threads in FP AddressSpaces really use FP all the time, for hard-float Rust
  (`first_use_traps` against `fp_entries`, with usage tracking);
- how much the eager baseline costs (`fp_saves`, `fp_loads` and the FP share of Thread
  switches);
- what the decay budget N should be, and whether owner tracking pays off (`decays` and
  `owner_hits`).

## Later: usage tracking with decay and owner tracking

The first implementation is plain eager save and restore, with no FP trap. Usage
tracking (SIMD-10, planned in P-SIMD-5) adds the following, and the first-use trap
comes with it.

**Usage flag with a decaying budget.**

- A Thread holds `fp_budget: u8`. A Thread with a budget of zero enters FP frames with
  `FPEN` trapping and nothing loaded.
- The first FP instruction then traps with EC 0x07 in an FP AddressSpace. The kernel
  sets `fp_budget = N`, loads the pending state, enables FP and re-runs the instruction.
- Each later eager entry into an FP frame decrements the budget. When it reaches zero,
  the next entry traps again.

While FP is enabled the kernel cannot see whether a Thread uses it. Re-arming the trap
every N entries is how it notices that the Thread stopped. A Thread that keeps using FP
pays one trap every N entries. N comes from the eager baseline's counters.

**Pending state lives in `Thread.fp`.** A Thread whose budget is zero may still own FP
state. That is the case when its budget decayed, when a Return owes it `q8..q15` and
FPCR, or when a fault Return owes it the frame's FP part. So the kernel never defers a
load-from-elsewhere. It writes the owed state into `Thread.fp` first:

| Owed state | What `Thread.fp` gets |
|---|---|
| Entry at a Call | Zeros |
| Return | The record's `q8..q15`, FPCR and FPSR, the rest zeroed |
| Fault Return | The validated fault frame's FP part, from the scratch |

The first-use trap always loads `Thread.fp`. This costs one memory copy in place of a
register write, and keeps a single load path.

**Owner tracking.**

- The nucleus records which (Thread, call depth) frame's state the hardware holds, if
  any. On one core this is one field; under multicore, one per core.
- Re-entering that same frame, with no other FP frame run in between, skips both the
  save and the load (`owner_hits`). Integer-only frames don't count, because they
  cannot touch the registers.
- The owner record is cleared when its Thread or AddressSpace is retired, and when a
  Return pops the owning depth.
- Under multicore it is also cleared, and the state saved, before the Thread runs on
  another core.

**Changes to the invariants.**

- SIMD-14 becomes: FP is enabled only in an FP frame of a Thread whose budget is
  non-zero.
- SIMD-15 becomes: the register file was either written in this entry or is still
  owned by this frame.

The model check gains the trap, the budget and the owner record as extra steps.

## Correctness argument

### Invariants

These hold at every `eret` to EL0, and at every return to an `EL1t` Thread.

#### SIMD-14: the gate

`FPEN` permits FP if and only if the frame being entered is an FP frame.

#### SIMD-15: full write on entry

Before an FP frame is entered, the whole register file has been written in the
same kernel entry. The values come from one of four places:
- the frame's own saved state;
- zeros;
- its `q8..q15`, FPCR and FPSR from its own call record, with the rest zeroed;
- for a fault Return, the FP part of its validated fault frame.

#### SIMD-16: kernel abstinence

Kernel code executes no FP instruction outside the `FpUnit` routines. The
soft-float build and `_audit-fp-simd` enforce this, and the entry guard
(SIMD-12) adds a hardware check in debug kernels.

#### SIMD-17: save before overwrite

When an FP frame stops, everything it may need back is copied out in the same
kernel entry, before anything overwrites the register file: everything at a
Thread switch or a fault, and `q8..q15`, FPCR and FPSR at a Call. By SIMD-16,
the copy sees exactly the frame's values.

### Lemmas

- **Confidentiality.** FP registers are readable only in an FP frame (SIMD-14). Every entry
  into an FP frame first overwrites the whole register file with that frame's own
  values or with zeros (SIMD-15). So no frame observes another frame's values in the
  registers. This covers other Threads, a callee looking at its caller, and a caller
  looking at its callee. The one deliberate exception is the fault frame: the faulting
  AddressSpace's own handler sees the faulting state in memory, by design (SIMD-9).
- **Integrity.** An FP frame gets back what it is owed:
  - after preemption, blocking or a fault: its full state (SIMD-17 and the load);
  - after a Call returns: `q8..q15`, FPCR and FPSR as they were and the rest zero, which
    AAPCS64 permits for a call.

### Edge cases

Each of these satisfies SIMD-14 to SIMD-17.

- **FP → integer-only → FP, including A→B→A.** The inner A frame is entered zeroed. The
  B→A record stores nothing, because B is integer-only, and its bit says so. The Return
  into B traps. The outer A frame's preserved registers come from the A→B record.
- **Blocking inside an integer-only callee above an FP caller.** Nothing is saved. The
  call clobbers the caller's non-preserved registers anyway, and its preserved ones are
  already in its record.
- **Blocking inside an FP callee.** A full save to `Thread.fp`, and a full load when it
  resumes.
- **Preemption at any instruction of an FP frame.** A full save. Interrupt handling
  touches no FP (SIMD-16).
- **A fault in an FP frame, with the handler in an FP or an integer-only AddressSpace.**
  - The full state goes into the fault frame in the handler pool's fault area. The
    handler is entered zeroed or trapped. If the pool has no fault area, the fault is
    unhandled.
  - The handler's Return at the fault depth copies the frame back, validates the copy
    and loads its FP part. If it is invalid, the Thread is terminated. With Terminate,
    the frame stays in its slot.
  - A fault inside the handler is unhandled, as today, and parks the Thread.
- **A Thread retired, terminated or exiting while its values are live in hardware.**
  Nothing is saved. The values are unreadable (SIMD-14) and are overwritten at the next FP
  entry (SIMD-15).
- **The caller's AddressSpace retired before Return (`ReturnTargetRetired`).** Nothing
  is loaded. The existing fault path runs, and SIMD-14 and SIMD-15 hold for whatever
  frame comes next.
- **`EL1t` fixture Threads.** They run in integer-only AddressSpaces, so SIMD-14 traps
  EL1 again on entry, and `fp-trap-test`'s EL1t check stays valid.
- **The kernel running with `FPEN` = `0b11` after an entry from an FP frame.** In a
  release kernel, the build and the audit are the only guarantee of SIMD-16. This is the
  weakest point of the design, which is why the audit fails when it scans nothing (see
  "The model"). In a debug kernel, the entry guard makes the hardware trap a stray kernel
  FP instruction. In both cases the audit stays part of `just lint`.
- **The first start of a Thread.** `Thread.fp` is zero from the carve, and the first
  entry loads it.
- **`Activate`, CallWithKey, `Scheduler.Dispatch` and `Thread.Suspend`.** These reduce
  to rows of the transition table: no change, a Call, and a Thread switch.
- **Multicore.** The save is eager at the switch away, so a Thread's FP state is
  never left live on another core, and migrating a Thread needs nothing extra. A
  lazy-ownership optimisation would give this up.

### Proof obligations

- This argument becomes the text of the contract change.
- **Bounded model check, a host test.** The system is small:
  - 2 Threads;
  - AddressSpaces FP-A, FP-B and integer-only C;
  - Call depth up to 3.

  The test enumerates every sequence of up to about 8 transitions. Each step is a Call,
  Return, block, resume, preempt, fault, handler Return or retire. The steps run through
  the real object-layer code against a fake `FpUnit` that tags every value with the
  frame that wrote it. After each step the test asserts:
  - SIMD-14;
  - that EL0 observes only its own frame's tags or zero;
  - that each resumed frame holds the tags it is owed.
- **End-to-end `fp-test` kernel,** in QEMU and on the Pi 4.

## Building and loading FP components

- **A second target spec,** `targets/aarch64-metta-none-hardfloat.json`: hard-float
  ABI, `+neon,+fp-armv8`. Soft-float and hard-float objects cannot be linked together,
  so shared libraries are built for both targets. Cargo keeps them in separate
  `target/<spec>/` directories.
- **The Justfile builds FP components separately (SIMD-5).** An `fp_components` list
  is built by a `_cross-build` that takes the target spec as a parameter. A marker in
  the crate itself is future work (N-SIMD-1).
- **One Rust checker, `fp-scan`.** It is a host crate under `tools/`, with a library and
  a binary.
  - It reads an ELF and tests every 32-bit word of its executable segments against the
    AArch64 encoding classes, skipping data that the `$d` mapping symbols mark in code:
    - FP and SIMD data processing, and loads and stores of FP or SIMD registers: the
      words with A64 bits 27 and 26 both set, which no other class has;
    - SVE;
    - the `mrs`/`msr` encodings for FPCR and FPSR.
  - It reports each hit with the symbol it falls in.
  - **The image-build library uses it (SIMD-6).** When `libs/image-build` packs a
    component, a hit sets `ComponentImage`'s FP flag. `Builder::component`
    (`libs/component-loader/src/component.rs`) takes the image and passes its flag to
    Retype's `x3`, so no loader can create a component's AddressSpace with the wrong
    flag. No manifest key selects FP.
  - **The scan is checked against `fp_components`.** The Justfile exports the list to
    image-build (`VESPER_FP_COMPONENTS`), which also finds those binaries in the
    hard-float target's directory.
    - A component that the scan finds FP in, but that is not on the list, fails the
      build. A component built hard-float by mistake, or one with stray FP assembly,
      therefore fails instead of quietly becoming an FP component.
    - A listed component with no hits is allowed. It runs trapped and never traps.
    - fp-trap-test's probe executes one FP instruction to show that it traps. Its
      manifest entry says so (`expects_fp_trap = true`): its hits are allowed, it stays
      integer-only, and a probe without hits fails the build.
  - **`_audit-fp-simd` runs the binary,** in place of the earlier `rust-objdump` and
    `grep` pipeline. It rejects any hit in an integer-only image. It allows hits in the
    kernel images only inside the `fp_unit_*` routines, matched by symbol name. It does
    not audit the hard-float components, and requires hits in fp-trap-test's probe, as a
    check that the scan still finds what it must.
  - The flag and the audit then cannot disagree, because they are one check.
- **`libsyscall`.** Under `cfg(target_feature = "neon")`, `ppc_call` and
  `ppc_call_with_key` declare `v0..v7` and `v16..v31` as clobbered, which are the AAPCS
  call clobbers. Every other syscall preserves the whole register file, because the
  kernel does not touch it.

## Where the selected contract lives

The FP/SIMD rules are SIMD-1 to SIMD-18 above. The parts that are capability
wire rules stay in `capabilities-contract.md`:

- **The Retype `x3` schema:** the AddressSpace flag in bit 8.
- **The Call register table:** the FP rows, and the call-record size.
- **"Fault delivery":** the fault frame.
- **The scheduler-record layouts:** the FP counters.
- **The decision register:** D7.

`audio-design.md` option 4 points here. The synthesiser stays integer-only.
