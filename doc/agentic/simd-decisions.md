# FP/SIMD open decisions

This file parks the FP/SIMD decisions the maintainer has not answered yet. Each item
names the step that needs it. Do not assume an answer; record one only when the
maintainer gives it explicitly. An answered decision is recorded in [simd-design.md](simd-design.md) and listed in
[simd-completed.md](simd-completed.md), section Completed decisions. Tasks stay in
[simd-implementation-plan.md](simd-implementation-plan.md).

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

Status: the guard is built (S2) and measured on the Pi 4 with `fp-bench`; the numbers are
in [doc/simd-performance.md](../simd-performance.md). It adds 4.3 ns (0.3 %) to a
Signal, 79.6 ns (1.3 %) to a Call into an integer-only component and 149.0 ns (2.4 %) to
a Call into an FP component. The maintainer's decision is pending. The guard is an
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
