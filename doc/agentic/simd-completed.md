# FP/SIMD completed work

One line per finished item from [simd-implementation-plan.md](simd-implementation-plan.md).
The details live in the contract and the design, and the history in version control.

- [x] S0 contract: FP/SIMD per AddressSpace in `capabilities-contract.md`. This covers
  architectural state, the Retype `x3` bit 8 flag, the FP rows of the Call register table
  and the call-record size, the fault frame in fault delivery, the FP counters in the
  scheduler-record layouts, and D7 in the decision register. Brief planned-change notes
  are in `doc/object_types` (Invocation, Thread, AddressSpace, Scheduler). Documentation
  only; nothing is implemented.
- [x] S1 data layout: `FpState` (528 B, 16-aligned) and `FpPreserved` (136 B) in
  `libs/exception`. The nucleus gains `Thread.fp`, `Thread.fp_stats` (`Thread` is now
  16-aligned), and `InvocationContinuation.source_fp` with its `fp_preserved` bit
  (312 B per record). Exercised by the `exception` device tests and the nucleus size
  asserts.
- [x] S1 counter storage: `ThreadFpCounters` and `FpTotals` are named in the
  `ThreadRecord` and `SchedulerRecord` layouts (`libs/object/src/scheduler_record.rs`) and
  published by the nucleus, with the totals going to the root Scheduler only. Nothing
  counts yet. Exercised by the `scheduler_record` host tests (`just _test-object-host`).
- [x] S1 AddressSpace flag: `AArch64AddressSpace.fp_simd`, set from bit 8 of Retype's
  `x3` (`ADDRESS_SPACE_FP_SIMD_BIT`), with bits 63:9 rejected. Userspace gets
  `UntypedKey::retype_address_space`. The flag gates nothing yet. Exercised by
  `retype_address_space_takes_the_fp_flag_and_rejects_higher_bits` (`just _test-untyped`).
- [x] S1 transition logic: `FpUnit` (`kernel/nucleus/src/objects/fp.rs`), reached
  through `ArchObjects::Fp`. The kernel uses `TrappedFpUnit`, a no-op until S2, and the
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
- [x] S1 fault frame (F8(d), F8.1–F8.3(iii); F12 provisionally (a)): `FaultFrame`
  (832 B, `libs/object/src/fault.rs`) written at the top of the handler's pool stack
  through `page_table::user_page` and the kernel direct map, its address in `x0` and SP
  below it; read back, validated and resumed at Retry or Skip, FP part included.
  Exercised by the nucleus fault tests (`just _test-debug-console`) and fault-test's
  emulating handler (`just _test-fault`).
- [x] S1 model check extended with fault delivery, the handler's Return and Thread
  exit: every 5-step sequence of five step kinds. It was shown to fail on a
  deliberately missing fault-frame save.
