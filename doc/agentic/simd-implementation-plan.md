# FP/SIMD implementation plan

Companion to [simd-design.md](simd-design.md), which holds the design and its
correctness argument. [simd-decisions.md](simd-decisions.md) holds the open decisions.

This file holds only open work, in order. Each step is verified before the next one
starts. Mark an item `- [x]` only after its verification has passed. A finished item
leaves this file as a one-line marker in [simd-completed.md](simd-completed.md).

Done: S0 (the contract), S1 (object layer) and S2 (routines and switch hooks); see
[simd-completed.md](simd-completed.md).

When an S1 to S4 step implements part of the design, it also rewrites the matching
"Planned changes" note in `doc/object_types` into the page's normal description, and
drops the "not implemented" status from the contract text it fulfils.

## S1: object layer and model check

Done:
- the FP data layout;
- the counter storage and its publication;
- the AddressSpace flag;
- `FpUnit`;
- the transition logic for Thread switches, Call and Return, with its counting;
- the row tests and the bounded model check;
- the fault frame, and the model check's fault, handler-Return and exit steps.

See [simd-completed.md](simd-completed.md).

- Verify: the nucleus tests, `just clippy`, and `just _test-fp-trap`, which still traps,
  since no hardware FP is enabled yet.

## S2: aarch64 routines and the switch hooks

Done; see [simd-completed.md](simd-completed.md). F11 stays open until S4 measures the
guard.

## S3: build side

- [ ] Add the target spec `targets/aarch64-metta-none-hardfloat.json`, and pass the
  target spec as a parameter to `_cross-build`. Build the crates of an `fp_components`
  list with it (F5.1(a)).
- [ ] `tools/fp-scan`, a host crate with a library and a binary:
  - an ELF reader that tests every word of the executable segments against the AArch64
    FP/SIMD encoding classes and the FPCR and FPSR accesses, and reports each hit with
    its symbol;
  - host tests with known FP, integer-only and literal-pool words.
- [ ] Image-build derives `ComponentImage`'s FP flag with the `fp-scan` library
  (F5.2(a)), and carries it through `libs/image` into `Builder::component`.
- [ ] The Justfile passes `fp_components` to image-build. A component with FP hits that
  is not on the list fails the build. Test it with an integer-only test component that
  contains one FP instruction and is not listed.
- [ ] `_audit-fp-simd` runs the `fp-scan` binary in place of `rust-objdump` and `grep`:
  - integer-only images must have no hits;
  - the nucleus may have hits only in the allow-listed routines;
  - flagged FP components and fp-trap-test's probe are skipped.

  Check that it still rejects what the old check rejected, using fp-trap-test's probe.
- [ ] `libsyscall`: under `cfg(target_feature = "neon")`, declare `v0..v7` and
  `v16..v31` clobbered by `ppc_call` and `ppc_call_with_key`.
- Verify: the integer-only images build unchanged, an FP component builds, and
  `just lint` passes.

## S4: end-to-end

- [ ] An `fp-test` kernel, after `kernel/tests/fp-trap-test`, that checks:
  - two FP components, each preempted mid-loop with sentinel values in its V registers;
  - a Call from one FP component into the other: the callee sees zeros, and the caller
    gets back `q8..q15` and FPCR;
  - a Call into an integer-only component, where an FP instruction traps;
  - an FP fault that is handled, then resumed with its V registers intact.

  Run it as `just _test-fp` in QEMU.
- [ ] Run it on the Pi 4, as part of board-test or as its own image.
- [ ] Measure the F11 overhead on the Pi 4. Run the same FP workload (`fp-test`, or a
  syscall-heavy FP component) with `fp_entry_trap` on and off. Compare syscall round
  trips and the FP statistics. Record the numbers, so the maintainer can decide whether
  the guard stays (F11).

## S5: measure, then the less eager variant

S1 to S4 are plain eager save and restore. S5 starts only after their statistics have
been measured on the board.

- [ ] Measure the eager baseline with the FP statistics (from S1 and S2) on a
  representative FP workload, and record the numbers.
- [ ] The usage flag with a decaying budget, and the first-use trap that comes with
  it, as the design's "Later" section describes.
  - The owed state is written into `Thread.fp` before the trap.
  - Choose N from the measurements.
  - Update invariants I1 and I2, the model check and `fp-test`.
- [ ] Owner tracking: remember the (Thread, call depth) frame whose state the hardware
  holds, so FP → integer-only → the same FP frame skips the save and the load. On one
  core this is a field next to `current_thread`, and per-core once there is multicore.
- [ ] Measure again and compare with the baseline, using `first_use_traps`, `decays`
  and `owner_hits`.
- [ ] Shrink the Thread, now that its size is accepted (F7(a)): for example, keep the
  FP part of call records in a side table used only by FP callers.
