# FP/SIMD implementation plan

Companion to [simd-design.md](simd-design.md), which holds the decisions and the
correctness argument. No FP/SIMD decision is open; future work is at the end of this
file.

This file holds only open work, in order. Each step is verified before the next one
starts. Mark an item `- [x]` only after its verification has passed. A finished item
leaves this file as a one-line marker in [simd-completed.md](simd-completed.md).

Done: P-SIMD-0 (the contract), P-SIMD-1 (object layer), P-SIMD-2 (routines and
switch hooks), P-SIMD-3 (build side) and P-SIMD-4 (end to end, with SIMD-12);
see [simd-completed.md](simd-completed.md).

## P-SIMD-5: measure, then the less eager variant

The first implementation is plain eager save and restore. This starts only after
its statistics have been measured on the board (SIMD-10).

- [ ] Measure the eager baseline with the FP statistics on a representative FP
  workload, and record the numbers. The workload is `midi-synth-fp`,
  the MIDI synthesiser with its f64 engine (AUDIO-20): `_test-midi-fp` in QEMU,
  midi-test with the `fp-mode` feature on the board. On its integer engine (the engine
  packet) the same component is an FP AddressSpace that does little or no FP, the case
  the decaying flag is for; how little depends on what the compiler vectorized in its
  integer code, to be checked when this is measured.
- [ ] The usage flag with a decaying budget, and the first-use trap that comes with
  it, as the design's "Later" section describes.
  - The owed state is written into `Thread.fp` before the trap.
  - Choose N from the measurements.
  - Update SIMD-14 and SIMD-15, the model check and `fp-test`.
- [ ] Owner tracking: remember the (Thread, call depth) frame whose state the hardware
  holds, so FP → integer-only → the same FP frame skips the save and the load. On one
  core this is a field next to `current_thread`, and per-core once there is multicore.
- [ ] Measure again and compare with the baseline, using `first_use_traps`, `decays`
  and `owner_hits`.
- [ ] Shrink the Thread, now that its size is accepted (SIMD-8): for example, keep the
  FP part of call records in a side table used only by FP callers.

## Future work

Ideas that might be pursued later. They need no decision now and constrain nothing
current.

- **N-SIMD-1: a crate marker for hard-float builds.** A crate declares itself
  FP, for example with `[package.metadata.vesper] fp_simd = true`, and the Justfile
  derives its `fp_components` list from `cargo metadata`. The crate then states its own
  nature, and the list cannot drift from it.
- **N-SIMD-2: checking the scan against the crate marker.** Once N-SIMD-1 exists,
  image-build compares the `fp-scan` result with each crate's marker instead of with
  the Justfile list. A component with FP hits but no marker fails the build.
