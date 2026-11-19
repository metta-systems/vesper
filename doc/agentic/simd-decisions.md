# FP/SIMD open decisions

This file parks the FP/SIMD decisions the maintainer has not answered yet. Each item
names the step that needs it. Do not assume an answer; record one only when the
maintainer gives it explicitly. An answered decision moves into
[simd-design.md](simd-design.md). Tasks stay in
[simd-implementation-plan.md](simd-implementation-plan.md).

Answered, and recorded in [simd-design.md](simd-design.md):

- F1(a), 2026-11-18. Scope: FP is opt-in, chosen per component.
- F2(a), 2026-11-19. The right to execute FP comes from the AddressSpace, fixed at its
  creation.
- F3, 2026-11-19. The first implementation (S1–S4) is plain eager save and restore.
  Later, in S5, a Thread that uses FP is tracked and restored eagerly only while it is
  in an FP AddressSpace.
- F4(a2), 2026-11-19. A Call enters the target with the register file zeroed, and
  Return restores `q8..q15`, FPCR and FPSR from the call record.
- F5.1(a), 2026-11-19. A Justfile list chooses which crates build hard-float. The crate
  marker, (b), is future work; see below.
- F5.2(a), 2026-11-19. Image-build derives the runtime FP flag by scanning the ELF. The
  same Rust checker replaces the disassembly in `_audit-fp-simd`. The scan is checked
  against `fp_components`, and a component with FP hits that is not on the list fails
  the build.
- F6(b), 2026-11-18. The fault handler can inspect the faulting frame's FP state. F8 is
  how.
- F7(a), 2026-11-18. The Thread grows to about 6 KiB, with everything inline, and is
  shrunk in the measured pass.
- F9, 2026-11-19. The S5 variant uses a decaying usage flag together with owner
  tracking, and the first-use trap arrives with it. FP statistics are counted per
  Thread from S1 on.
- F10(b), 2026-11-19. The FP statistics are published in the scheduler-shared records.
  The root scheduler can read them and decide on them, which leaves room for an adaptive
  FP policy later.
- F8(d), 2026-11-19. A fault handler gets a fault frame on its own stack, with the
  frame's address in `x0`.
  - F8.1: the frame holds the faulting frame's general registers as well as its FP
    state.
  - F8.2: the handler may write the frame back, and a Retry or Skip resumes from it.
    This needs no new trust: the handler can only change the state of the Thread it is
    handling, and that trust already exists. The kernel still checks the written-back
    SPSR, because that check protects the kernel, not the Thread.
  - F8.3(iii): the kernel keeps the general registers in `ThreadFault.frame`, as today,
    and the FP state lives only in the frame. There is no `ThreadFault.fp`. A frame that
    is unreadable or invalid at the Return terminates the Thread, with the kernel's
    general registers as its `Faulted` state.

## F11. FP off on kernel entry (needed by S2; decided after measuring)

AArch64 has no `FPEN` value that allows EL0 while trapping EL1. While an FP frame runs,
a kernel entry therefore starts with FP enabled. If the kernel then executed an FP
instruction, it would silently corrupt the frame's live registers.

- **(a)** Turn FP off at every entry. In `vectors.S`, after the general registers are
  saved, read `CPACR_EL1`. If FP is enabled, set `FPEN` to trap and execute `isb`.
  Before each `eret` into an FP frame, set `FPEN` to `0b11` and execute `isb`.
  - The kernel then always runs with FP trapped, except inside its own save, load and
    zero routines, which open `FPEN` themselves. A stray kernel FP instruction traps at
    EL1 instead of corrupting state.
  - Cost: one `CPACR_EL1` write and one `isb` per entry from an FP frame and per return
    into one. It is probably a few tens of cycles on an A72, but this has not been
    measured.
- **(b)** Leave FP enabled during kernel entries from FP frames. The soft-float build
  and `_audit-fp-simd` remain the only guard.

In both options `_audit-fp-simd` stays mandatory in `just lint`, nucleus included.

The maintainer's direction on 2026-11-19 is to measure first:

- The guard is built behind a nucleus feature, `fp_entry_trap`.
- The same FP workload runs with the feature on and with it off.
- Then the maintainer decides whether to keep it or remove it.

Status: the guard is built (S2), and the measurement waits for S4's FP workload. It is an
`FpEntryGuard` held by every exception handler whose entry can return to a Thread
(`lower_aarch64_*` and `current_el0_*` in `kernel/nucleus/src/main.rs`), not code in
`vectors.S`. The effect is the same, since the handler prologue before it is soft-float,
and the `CPACR_EL1` access stays typed. `_test-fp-entry-trap` shows a kernel FP
instruction trapping at EL1 while serving an FP frame, and `_test-fp-trap` shows it
executing without the guard. The toggles are counted in the root Scheduler record's
`fp_entry_toggles`.

The audit is expected to catch any stray kernel FP instruction, since the kernel is
small.

## F12. The fault frame conflicts with the stack-pool rule (for review before S1)

A side review found this, and it is recorded for the maintainer's review. The stack-pool
decision of 2026-10-19 (`capabilities-contract.md`, "Stack pools") says the pool's stack
memory is the exporter's: "the kernel never reads or writes it and does not prove it
mapped". The fault frame (F8(d)) has the kernel write the faulting state at the top of a
fault handler's pool stack, and read it back at the handler's Return. Components may
rely on the kernel leaving their stacks alone, so this is a change to a guarantee.

S0 recorded the fault frame under "Fault delivery" but left the "Stack pools" text as it
was, so the contract now contradicts itself there. Whichever way this is settled, the
"Stack pools" section must be rewritten openly, as a dated amendment, and not changed
silently.

- **(a)** Amend the stack-pool rule. The kernel never touches pool stacks, except for
  the fault frame at the top of the stack a fault delivery takes. That frame is written
  and read only after a software walk of the handler's translation tables checks each
  page is EL0-accessible, through the kernel's direct map. An exporter whose pool
  serves a fault handler sizes its stacks for the 832 B frame plus the handler's own
  use. **Implemented provisionally as (a)** (maintainer, 2026-11-19: "for now continue
  as if it was F12 (a)"); the question stays open for later review, and the contract's
  "Stack pools" text carries the amendment marked provisional.
- **(b)** Keep the rule, and put the frame somewhere else: a save area the handler's
  AddressSpace registers for fault delivery. That needs a new operand or operation, and
  a rule for several faulting AddressSpaces that share one handler.

## Future work

Ideas that might be pursued later. They need no decision now and constrain nothing
current.

- **A crate marker for hard-float builds** (formerly F5.1(b)). A crate declares itself
  FP, for example with `[package.metadata.vesper] fp_simd = true`, and the Justfile
  derives its `fp_components` list from `cargo metadata`. The crate then states its own
  nature, and the list cannot drift from it.
- **Checking the scan against the crate marker.** Once the marker exists, image-build
  compares the `fp-scan` result with each crate's marker instead of with the Justfile
  list. A component with FP hits but no marker fails the build.
