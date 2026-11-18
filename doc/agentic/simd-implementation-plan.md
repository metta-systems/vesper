# FP/SIMD implementation plan

Companion to [simd-design.md](simd-design.md), which holds the design and its
correctness argument. [simd-decisions.md](simd-decisions.md) holds the open decisions.

This file holds only open work, in order. Each step is verified before the next one
starts. Mark an item `- [x]` only after its verification has passed. A finished item
leaves this file as a one-line marker in [simd-completed.md](simd-completed.md).

Done: S0, the contract; see [simd-completed.md](simd-completed.md).

When an S1 to S4 step implements part of the design, it also rewrites the matching
"Planned changes" note in `doc/object_types` into the page's normal description, and
drops the "not implemented" status from the contract text it fulfils.

## S1: object layer and model check (host)

- [ ] `FpState` (528 B, 16-aligned) and `FpPreserved` (`q8..q15`, FPCR and FPSR,
  F4(a2)). Add `Thread.fp`, a per-core 816 B fault scratch, `InvocationContinuation.source_fp` and its
  "FP preserved" bit, and update the size asserts in `objects/thread.rs`. Everything is
  inline (F7(a)).
- [ ] FP statistics, as the design's "FP statistics" section lists them:
  `Thread.fp_stats` and the totals in the nucleus, counted in the transition logic.
  They are published into `ThreadRecord`'s former reserved words at the switch away,
  and the totals into the root `SchedulerRecord` (`libs/object/src/scheduler_record.rs`,
  F10(b)). Host tests check the counts for each row of the transition table, and the
  published record.
- [ ] The fault frame (F8(d)), as the design's "The fault frame" describes:
  - it holds the general registers and, for an FP frame, the FP state;
  - it is written to the handler's pool stack with checked `STTR`, and its address is
    passed in `x0`;
  - at Retry and Skip it is read back once into per-core scratch, validated (SPSR at
    EL0t) and resumed from;
  - an unreadable or invalid frame terminates the Thread.

  Host tests cover a modified frame, a bad SPSR, an unwritable stack and an unreadable
  frame at Return. The FP state lives only in the frame, and the kernel keeps only the
  general registers (F8.3(iii)).
- [ ] `AArch64AddressSpace.fp_simd`, set from bit 8 of Retype's `x3` and passed to
  `new_address_space`. Reject the bit for every other kind. Update `AddressSpaceKey`'s
  Retype helper in `libs/object`.
- [ ] Add an `FpUnit` trait next to `install_translation_context`
  (`objects/arch_objects.rs`), and a fake implementation for host tests.
- [ ] Transition logic in `stop_current`, `commit_entry`, `commit_call`,
  `commit_return` and `deliver_fault`, as the transition table in the design gives it.
- [ ] Host tests:
  - one per row of the transition table;
  - no FP leak across Call, Return, fault or Thread switch, after the `TPIDR_EL0` tests
    in `kernel/nucleus/tests/support/invocation_call.rs`;
  - the exhaustive bounded model check from the design's "Proof obligations".
- Verify: the nucleus host tests, `just clippy`, and `just _test-fp-trap`, which still
  traps, since no hardware FP is enabled yet.

## S2: aarch64 routines and the switch hooks

- [ ] Write `save`, `load`, `zero`, `save_preserved` and `load_preserved` in
  `global_asm!` under `.arch_extension simd`, with `FPEN` opened around them.
- [ ] Set `CPACR_EL1.FPEN` next to each of the five TTBR0 writes in
  `kernel/nucleus/src/main.rs`.
- [ ] Give `_audit-fp-simd` an allow-list for the nucleus's FP routines, matched by
  symbol name. This uses `fp-scan` once S3 has it; until then, the existing `objdump`
  check with the allow-list.
- [ ] The F11 entry guard, behind a nucleus feature `fp_entry_trap`:
  - turn FP off at kernel entry from an FP frame in `vectors.S`, and on again before
    the `eret` into one;
  - count the toggles in the FP statistics;
  - add an `fp-trap-test` case, built with the feature, in which kernel FP use while
    serving an FP component traps at EL1.
- Verify: `just lint` passes, and so do `just _test-fp-trap`, `just _test-ppc` and
  `just _test-fault`.

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
