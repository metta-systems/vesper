# FP/SIMD design

Status: the contract is recorded in `capabilities-contract.md` (S0), and nothing is
implemented.

- Decided by the maintainer:
  - F1(a), 2026-11-18: FP/SIMD is opt-in, chosen per component.
  - F2(a), 2026-11-19: the right to execute FP comes from the AddressSpace and is fixed
    at its creation.
  - F3, 2026-11-19: the first implementation, S1–S4, is plain eager save and restore.
    Later, in S5, a Thread that uses FP is tracked and restored eagerly only while it is
    in an FP AddressSpace.
  - F4(a2), 2026-11-19: a Call enters the target with the register file zeroed, and
    Return restores `q8..q15`, FPCR and FPSR.
  - F5.1(a), 2026-11-19: a Justfile list chooses which crates build hard-float. A
    marker in the crate is future work.
  - F5.2(a), 2026-11-19: the runtime FP flag is derived by scanning the ELF. The Rust
    checker that does it also replaces the disassembly in `_audit-fp-simd`. A component
    with FP hits that is not in `fp_components` fails the build.
  - F6(b), 2026-11-18: a fault handler can inspect the faulting frame's FP state.
  - F8(d), 2026-11-19: the handler gets the faulting state as a fault frame on its own
    stack.
    - F8.1: the frame includes the general registers.
    - F8.2: the handler may write the frame back.
    - F8.3(iii): the kernel keeps only the general registers, and the FP state lives
      only in the frame. See "The fault frame".
  - F7(a), 2026-11-18: the Thread holds everything inline, at about 6 KiB, and shrinks
    later in the measured optimisation pass.
  - F9, 2026-11-19: S5 uses a decaying usage flag together with owner tracking, and the
    first-use trap arrives with it. FP statistics are counted per Thread from S1 on.
  - F10(b), 2026-11-19: the FP statistics are published in the scheduler-shared
    records. They are scheduling information, which the root scheduler can read and act
    on, so the FP policy can become adaptive later.
- The other open decisions are in [simd-decisions.md](simd-decisions.md).
- The work order is in [simd-implementation-plan.md](simd-implementation-plan.md).

## Scope

Today execution is integer-only, as `capabilities-contract.md` says under
"Architectural state → FP/SIMD". It is enforced at four points:

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

   F11 proposes turning FP off in `vectors.S` at every entry from an FP frame, and back
   on before the `eret` into one. The kernel would then always run trapped, outside its
   own routines. It is built behind a nucleus feature, `fp_entry_trap`. The overhead is
   measured with the feature on and off before the maintainer decides whether it stays.

   Either way, `_audit-fp-simd` stays mandatory in `just lint`, including the nucleus
   with an allow-list naming the three routines.

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
FPSR holds the cumulative exception flags. It is preserved too (F4(a2)), so a caller
that tests its flags across a call keeps them. The record is 128 + 4 + 4 bytes, the
136 B in the table. Every value is restored from the caller's own record, so nothing
from the callee leaks back.

A fault interrupts the frame between two arbitrary instructions, so every V register
is live, and its full FP state has to be kept. Under F8.3(iii) the kernel keeps only the
general registers, in `ThreadFault.frame`, as it does today. The FP state's only copy is
the fault frame on the handler's stack, described below. There is no `ThreadFault.fp`.

### The fault frame

The handler receives the faulting state as a frame on its own stack (F8(d)).

- **Where it lives.** The kernel writes the frame at the top of the stack that the
  forced Call takes from the handler Invocation's pool.
  - The stack is leased to this one call, survives Thread switches while the handler
    runs, and is released at the handler's Return.
  - The handler starts with SP below the frame.
  - `x0` holds the frame's address. A fault delivery is a plain Call, so `x0` would
    otherwise be zero. `FaultInfo` stays in `x2..x7`.
- **What it holds** (`libobject::fault::FaultFrame`, 832 B, 16-aligned):
  - a header with size, version and flags, including whether an FP part is present;
  - the faulting general registers (F8.1): `x0..x30`, SP, PC, `pstate`, `TPIDR_EL0`;
  - for an FP frame, the V registers, FPCR and FPSR.
- **How it is written.**
  - At delivery, `FpUnit::save` puts the faulting FP state into a kernel-stack copy of
    the frame, before the forced Call zeroes the registers for an FP handler.
  - The nucleus walks the handler AddressSpace's own translation tables in software
    (`page_table::user_page`), requires every page of the frame to be EL0-writable
    Normal memory with its access flag set, and writes through its direct map of
    physical memory. The handler AddressSpace need not be the active one. This is the
    first time the nucleus writes caller memory.
  - The write happens before the forced Call commits. A stack smaller than the frame,
    or not writable, makes the fault unhandled with nothing to undo, like a full
    invocation stack. The Thread becomes `Faulted` with its general registers, and its
    FP state is dropped.
  - The pool's stacks need room for 832 B plus the handler's own use.
- **Write-back (F8.2).** At a Retry or Skip Return, the kernel resumes from the frame,
  which the handler may have changed.
  - The kernel copies the frame once onto its own stack, through the same walk (pages
    EL0-readable), and validates that copy. It never validates in place, since another
    Thread of the handler's AddressSpace could change the frame in between.
  - The header must be unchanged, and `pstate` may differ from the faulting one only in
    the condition flags (NZCV), so the Thread resumes at the same EL with the same masks.
  - The FP part goes from the copy into the V registers with `FpUnit::load`.
  - An unreadable or invalid frame terminates the Thread. Its `Faulted` state is the
    kernel's copy of the general registers, `ThreadFault.frame`, as today. The original
    state is never silently resumed.
  - Terminate leaves the frame on the handler's stack for inspection.
- **Which copy counts (F8.3(iii)).**
  - The kernel keeps the general registers in `ThreadFault.frame`. That copy is the
    fallback for `Faulted` and the unhandled-fault path.
  - The FP state lives only in the frame.
  - A readable, valid frame is always what a Retry or Skip resumes, both its general
    registers and its FP state.
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
| `Thread.fp_stats`, plus `fp_budget` from S5 | 64 B, plus 1 B |

- The carve size follows the type (`carved_layout::<Thread>()`), so Untyped
  accounting adjusts by itself.
- The 176 B continuation assert in `kernel/nucleus/src/objects/thread.rs` must be
  updated.
- The size is accepted (F7(a)). Shrinking the Thread belongs to the measured
  optimisation pass that comes after correctness.

## Transitions

All five TTBR0 writes are in `kernel/nucleus/src/main.rs`. The supporting logic is in
`kernel/nucleus/src/objects/resume.rs`, `invocation.rs` and `fault.rs`.

| Transition | Site | Leaving an FP frame | Entering an FP frame | Entering integer-only |
|---|---|---|---|---|
| Thread switch: preemption, block, dispatch, idle wake, exit | `install_selected`; outgoing state captured in `stop_current`, next to the general registers | Save everything to the outgoing `Thread.fp` | Load the incoming `Thread.fp` | Trap |
| Call and CallWithKey | `commit_call` | Save `q8..q15`, FPCR and FPSR to the pushed record, and set its bit | Zero everything; FPCR is 0 | Trap |
| Fault delivery, a forced Call | `deliver_fault` | Save everything into the fault frame, written to the handler's stack before the Call commits; the record as for a Call | Zero | Trap |
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

The kernel counts from S1 on. The eager baseline is therefore measured before S5
changes anything. Every counter is a `u64` that wraps, and the kernel only increments
it.

**Per Thread.** The counters live in `Thread.fp_stats` and are published in the
Thread's scheduler-shared record (F10(b)). `ThreadRecord`
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
| `first_use_traps` | From S5: first-use traps taken |
| `decays` | From S5: times the usage flag decayed |
| `owner_hits` | From S5: saves and loads skipped because the hardware still held this Thread's frame |

**Totals.** These live in the nucleus next to `current_thread`, and are published in
the root Scheduler's `SchedulerRecord`, which also has eight reserved words:

- Thread switches;
- Thread switches that involved an FP frame on either side;
- with F11(a), the `FPEN` toggles at kernel entry and exit.

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
  (`first_use_traps` against `fp_entries`, from S5);
- how much the eager baseline costs (`fp_saves`, `fp_loads` and the FP share of Thread
  switches);
- what the decay budget N should be, and whether owner tracking pays off (`decays` and
  `owner_hits`).

## Later: usage tracking with decay and owner tracking (S5)

S1 to S4 are plain eager save and restore, with no FP trap. S5 adds the following, and
the first-use trap comes with it.

**Usage flag with a decaying budget.**

- A Thread holds `fp_budget: u8`. A Thread with a budget of zero enters FP frames with
  `FPEN` trapping and nothing loaded.
- The first FP instruction then traps with EC 0x07 in an FP AddressSpace. The kernel
  sets `fp_budget = N`, loads the pending state, enables FP and re-runs the instruction.
- Each later eager entry into an FP frame decrements the budget. When it reaches zero,
  the next entry traps again.

While FP is enabled the kernel cannot see whether a Thread uses it. Re-arming the trap
every N entries is how it notices that the Thread stopped. A Thread that keeps using FP
pays one trap every N entries. N comes from the S2 to S4 counters.

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

- I1 becomes: FP is enabled only in an FP frame of a Thread whose budget is non-zero.
- I2 becomes: the register file was either written in this entry or is still owned by
  this frame.

The model check gains the trap, the budget and the owner record as extra steps.

## Correctness argument

### Invariants

These hold at every `eret` to EL0, and at every return to an `EL1t` Thread.

- **I1, gate.** `FPEN` permits FP if and only if the frame being entered is an FP frame.
- **I2, full write on entry.** Before an FP frame is entered, the whole register file has
  been written in the same kernel entry. The values come from one of four places:
  - the frame's own saved state;
  - zeros;
  - its `q8..q15`, FPCR and FPSR from its own call record, with the rest zeroed;
  - for a fault Return, the FP part of its validated fault frame.
- **I3, kernel abstinence.** Kernel code executes no FP instruction outside the
  `FpUnit` routines. The soft-float build and `_audit-fp-simd` enforce this.
- **I4, save before overwrite.** When an FP frame stops, everything it may need back is
  copied out in the same kernel entry, before anything overwrites the register file:
  everything at a Thread switch or a fault, and `q8..q15`, FPCR and FPSR at a Call. By I3, the
  copy sees exactly the frame's values.

### Lemmas

- **Confidentiality.** FP registers are readable only in an FP frame (I1). Every entry
  into an FP frame first overwrites the whole register file with that frame's own
  values or with zeros (I2). So no frame observes another frame's values in the
  registers. This covers other Threads, a callee looking at its caller, and a caller
  looking at its callee. The one deliberate exception is the fault frame: the faulting
  AddressSpace's own handler sees the faulting state in memory, by design (F8).
- **Integrity.** An FP frame gets back what it is owed:
  - after preemption, blocking or a fault: its full state (I4 and the load);
  - after a Call returns: `q8..q15`, FPCR and FPSR as they were and the rest zero, which
    AAPCS64 permits for a call.

### Edge cases

Each of these satisfies I1 to I4.

- **FP → integer-only → FP, including A→B→A.** The inner A frame is entered zeroed. The
  B→A record stores nothing, because B is integer-only, and its bit says so. The Return
  into B traps. The outer A frame's preserved registers come from the A→B record.
- **Blocking inside an integer-only callee above an FP caller.** Nothing is saved. The
  call clobbers the caller's non-preserved registers anyway, and its preserved ones are
  already in its record.
- **Blocking inside an FP callee.** A full save to `Thread.fp`, and a full load when it
  resumes.
- **Preemption at any instruction of an FP frame.** A full save. Interrupt handling
  touches no FP (I3).
- **A fault in an FP frame, with the handler in an FP or an integer-only AddressSpace.**
  - The full state goes into the fault frame on the handler's stack. The handler is entered zeroed or trapped. If the frame cannot be
    written, the fault is unhandled.
  - The handler's Return at the fault depth reads the frame back, validates it and loads
    its FP part. If it is unreadable or invalid, the Thread is terminated. With
    Terminate, the frame stays on the handler's stack.
  - A fault inside the handler is unhandled, as today, and parks the Thread.
- **A Thread retired, terminated or exiting while its values are live in hardware.**
  Nothing is saved. The values are unreadable (I1) and are overwritten at the next FP
  entry (I2).
- **The caller's AddressSpace retired before Return (`ReturnTargetRetired`).** Nothing
  is loaded. The existing fault path runs, and I1 and I2 hold for whatever frame comes
  next.
- **`EL1t` fixture Threads.** They run in integer-only AddressSpaces, so I1 traps EL1
  again on entry, and `fp-trap-test`'s EL1t check stays valid.
- **The kernel running with `FPEN` = `0b11` after an entry from an FP frame.** Without
  F11(a), the build and the audit are the only guarantee of I3. This is the weakest point
  of the design. With F11(a), the hardware traps a stray kernel FP instruction. In both
  cases the audit stays part of `just lint`.
- **The first start of a Thread.** `Thread.fp` is zero from the carve, and the first
  entry loads it.
- **`Activate`, CallWithKey, `Scheduler.Dispatch` and `Thread.Suspend`.** These reduce
  to rows of the transition table: no change, a Call, and a Thread switch.
- **Multicore, later.** The save is eager at the switch away, so a Thread's FP state is
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
  - I1;
  - that EL0 observes only its own frame's tags or zero;
  - that each resumed frame holds the tags it is owed.
- **End-to-end `fp-test` kernel,** in QEMU and on the Pi 4.

## Building and loading FP components

- **A second target spec,** `targets/aarch64-metta-none-hardfloat.json`: hard-float
  ABI, `+neon,+fp-armv8`. Soft-float and hard-float objects cannot be linked together,
  so shared libraries are built for both targets. Cargo keeps them in separate
  `target/<spec>/` directories.
- **The Justfile builds FP components separately (F5.1(a)).** An `fp_components` list
  is built by a `_cross-build` that takes the target spec as a parameter. A marker in
  the crate itself is future work (see the decisions file).
- **One Rust checker, `fp-scan`.** It is a host crate under `tools/`, with a library and
  a binary.
  - It reads an ELF and tests every 32-bit word of its executable segments against the
    AArch64 encoding classes, skipping data that the `$d` mapping symbols mark in code:
    - FP and SIMD data processing, and loads and stores of FP or SIMD registers: the
      words with A64 bits 27 and 26 both set, which no other class has;
    - SVE;
    - the `mrs`/`msr` encodings for FPCR and FPSR.
  - It reports each hit with the symbol it falls in.
  - **The image-build library uses it (F5.2(a)).** When `libs/image-build` packs a
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

The selected contract is in `capabilities-contract.md`. This document is the design
discussion behind it.

- **"Architectural state":** FP per AddressSpace and invariants I1 to I4.
- **The Retype `x3` schema:** the AddressSpace flag in bit 8.
- **The Call register table:** the FP rows, and the call-record size.
- **"Fault delivery":** the fault frame.
- **The scheduler-record layouts:** the FP counters.
- **The decision register:** D7.

The user-facing pages in `doc/object_types` (Invocation, Thread, AddressSpace,
Scheduler) carry short planned-change notes until the implementation lands.
`audio-design.md` option 4 points here. The synthesiser stays integer-only.
