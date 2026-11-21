# FP/SIMD open decisions

This file parks the FP/SIMD decisions the maintainer has not answered yet. Each item
names the step that needs it. Do not assume an answer; record one only when the
maintainer gives it explicitly. An answered decision is recorded in [simd-design.md](simd-design.md) and listed in
[simd-completed.md](simd-completed.md), section Completed decisions. Tasks stay in
[simd-implementation-plan.md](simd-implementation-plan.md).

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
