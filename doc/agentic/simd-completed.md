# FP/SIMD completed work

One line per finished item from [simd-implementation-plan.md](simd-implementation-plan.md).
The decisions live in [simd-design.md](simd-design.md), the wire rules in the
contract, and the history in version control.

## P-SIMD-0: the contract

- [x] FP/SIMD per AddressSpace in `capabilities-contract.md`. This covers
  architectural state, the Retype `x3` bit 8 flag, the FP rows of the Call register table
  and the call-record size, the fault frame in fault delivery, the FP counters in the
  scheduler-record layouts, and D7 in the decision register. Brief planned-change notes
  are in `doc/object_types` (Invocation, Thread, AddressSpace, Scheduler). Documentation
  only; nothing is implemented.

## P-SIMD-1: object layer and model check

- [x] Data layout: `FpState` (528 B, 16-aligned) and `FpPreserved` (136 B) in
  `libs/exception`. The nucleus gains `Thread.fp`, `Thread.fp_stats` (`Thread` is now
  16-aligned), and `InvocationContinuation.source_fp` with its `fp_preserved` bit
  (312 B per record). Exercised by the `exception` device tests and the nucleus size
  asserts.
- [x] Counter storage: `ThreadFpCounters` and `FpTotals` are named in the
  `ThreadRecord` and `SchedulerRecord` layouts (`libs/object/src/scheduler_record.rs`) and
  published by the nucleus, with the totals going to the root Scheduler only. Nothing
  counts yet. Exercised by the `scheduler_record` host tests (`just _test-object-host`).
- [x] AddressSpace flag: `AArch64AddressSpace.fp_simd`, set from bit 8 of Retype's
  `x3` (`ADDRESS_SPACE_FP_SIMD_BIT`), with bits 63:9 rejected. Userspace gets
  `UntypedKey::retype_address_space`. The flag gates nothing yet. Exercised by
  `retype_address_space_takes_the_fp_flag_and_rejects_higher_bits` (`just _test-untyped`).
- [x] Transition logic: `FpUnit` (`kernel/nucleus/src/objects/fp.rs`), reached
  through `ArchObjects::Fp`. The kernel uses `TrappedFpUnit`, a no-op until P-SIMD-2, and the
  tests use `FakeFpUnit`.
  - The hooks, with the counters:
    - `stop_current` saves on a park or a preemption in an FP frame;
    - `commit_entry` loads, and counts the switch totals;
    - `commit_call` keeps `q8..q15`, FPCR and FPSR in the record, and zeroes for an
      FP target;
    - `commit_return` restores from the record.
  - Exercised by the five Call, Return and Thread-switch tests and the bounded model
    check in `kernel/nucleus/tests/support/invocation_call.rs` (`just _test-debug-console`).
    The model check covers both `AddressSpace`s with every combination of FP flags, two
    Threads, depth 3, and every 6-step sequence of Call, Return and preemption. It was
    shown to fail on a deliberately missing zeroing.
- [x] Fault frame (SIMD-9): `FaultFrame` (832 B,
  `libs/object/src/fault.rs`) handed to the handler, its address in `x0` (placed per
  SIMD-13 below); read back, validated and resumed at Retry or Skip, FP part included.
  Exercised by the nucleus fault tests (`just _test-debug-console`) and fault-test's
  emulating handler (`just _test-fault`).
- [x] Model check extended with fault delivery, the handler's Return and Thread
  exit: every 5-step sequence of five step kinds. It was shown to fail on a
  deliberately missing fault-frame save.

## P-SIMD-2: aarch64 routines and the switch hooks

- [x] Routines: `HardwareFpUnit`, six naked `fp_unit_*` functions in
  `kernel/nucleus/src/objects/arch/fp_unit.rs`, each opening `FPEN` around itself.
  Exercised against the real registers by the `fp_unit_tests` in `just _test-debug-console`.
- [x] Switch hooks: `install_translation_context` sets `FPEN` from the AddressSpace's
  flag (`PreparedTranslationContext::fp_simd`) at all five install sites. Exercised by
  `fp-trap-test`'s FP AddressSpace probe (`just _test-fp-trap`).
- [x] Audit: `_audit-fp-simd` skips only the `fp_unit_*` bodies, and its regex now
  matches unindented high-address lines, so it checks the kernel images at all (`just lint`).
- [x] Entry guard: `FpEntryGuard` behind `fp_entry_trap`, held by the `lower_aarch64_*`
  and `current_el0_*` handlers, toggles counted in `fp_entry_toggles`. Exercised by
  `just _test-fp-entry-trap` (kernel FP traps at EL1 serving an FP frame).

## P-SIMD-3: build side

- [x] Target: `targets/aarch64-metta-none-hardfloat.json`; `_cross-build` takes the
  target spec, and `_build-components` builds `fp_components` (`fp-worker`) with it.
  Linted by `_clippy-fp` in `just clippy`.
- [x] `tools/fp-scan`: encoding classes (bits 27:26, SVE, FPCR/FPSR), `$d` data skipped,
  hits with their symbol; library and binary. Host tests in `just _test-fp-scan-host`.
- [x] Flag: image-build sets `ComponentImage::fp_simd` from the scan and fails on FP in
  an unlisted component (`fp_simd_flag`, host-tested; shown on fp-probe without
  `expects_fp_trap`); `Builder::component` takes the image's flag.
- [x] Audit: `_audit-fp-simd` runs `fp-scan`, with `fp_unit_` allowed in kernel images
  and fp-probe required to have hits (`just lint`).
- [x] `libsyscall`: `ppc_call` and `ppc_call_with_key` clobber `v0..v7`, `v16..v31`
  under `target_feature = "neon"`.

## P-SIMD-4: end to end

- [x] `fp-test` (`kernel/tests/fp-test`, `fp-worker`, `fp-test-protocol`): sentinel
  spinners in two FP components across preemption, FP→FP and FP→integer-only Calls, a
  handled `brk`, checked in asm; `just _test-fp` (also passes with `fp_entry_trap`), shown
  to fail on a skipped register in `fp_unit_save` and in the zeroing.
- [x] `fp-test` on the Pi 4: `just boot-fp-test`, passes without and with
  `fp_entry_trap`.
- [x] Entry-guard measurement: `fp-bench` on the Pi 4 without and with `fp_entry_trap`,
  recorded in `doc/simd-performance.md`.
- [x] SIMD-12 guard: `debug_kernel` turns on `fp_entry_trap` (and the guard checks of
  fp-test, fp-bench and fp-trap-test); `just _test-fp` and `just _test-fp-bench` run as
  debug kernels, so with the guard.
- [x] SIMD-12 audit: `_audit-fp-simd` finds every soft-float image of the board set by
  itself (402, the release kernels included) and `fp-scan` refuses an audit that sees
  nothing (`fp_scan::audit`, host-tested); shown to fail on a blind decoder and on an
  unread symbol table.

## The fault area (SIMD-13)

- [x] SIMD-13 fault area: `CreateStackPool` `x5` registers a RAM Frame, slot `i` per
  stack `i` (`FAULT_SLOT_BYTES`); fault delivery writes the frame there through the
  kernel's mapping and leaves the stack alone (`objects/stack_pool.rs`, `objects/fault.rs`,
  `Builder::fault_handler_pool`). Exercised by the nucleus tests (`just
  _test-debug-console`), fault-test's refusals (`just _test-fault`), `just _test-fp` and
  `just _test-ppc`.
- [x] SIMD-13 walker kept: the EL0 page walk and copies moved, unused, to
  `kernel/nucleus/src/user_memory.rs`; `ArchObjects::copy_to_user`/`copy_from_user` are gone.
- [x] SIMD-13 on the Pi 4: `just boot-fp-test` passes without and with `fp_entry_trap`,
  the handled fault through the fault area included.
