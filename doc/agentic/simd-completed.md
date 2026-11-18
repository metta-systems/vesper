# FP/SIMD completed work

One line per finished item from [simd-implementation-plan.md](simd-implementation-plan.md).
The details live in the contract and the design, and the history in version control.

- [x] S0 contract: FP/SIMD per AddressSpace in `capabilities-contract.md`. This covers
  architectural state, the Retype `x3` bit 8 flag, the FP rows of the Call register table
  and the call-record size, the fault frame in fault delivery, the FP counters in the
  scheduler-record layouts, and D7 in the decision register. Brief planned-change notes
  are in `doc/object_types` (Invocation, Thread, AddressSpace, Scheduler). Documentation
  only; nothing is implemented.
