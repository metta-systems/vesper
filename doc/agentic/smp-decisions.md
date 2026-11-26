# SMP open decisions

Parking lot for unanswered SMP decisions. Each entry stays open until the maintainer
answers it explicitly; recommendations are marked as such and are not decisions. Answered
decisions are in [smp-design.md](smp-design.md#decisions); their tasks are in
[smp-implementation-plan.md](smp-implementation-plan.md).

`Scheduler.replace`, non-blocking dispatch and the activation invariant across cores stay
parked in [scheduling-decisions.md](scheduling-decisions.md); per-core
dispatchers do not need them.

## C11. Nucleus-wide totals in the Scheduler records

The root Scheduler's record carries totals the nucleus counts for the whole machine:
`interrupt_ns` (time spent handling interrupts), `idle_ns` (time with nothing to run), and,
from word 64, `thread_switches`, `fp_thread_switches` and `fp_entry_toggles`. With one root
Scheduler per core (C4(c)), where do these totals go?

- **(a) Per core.** Each core's root Scheduler record carries that core's own totals. Each
  core writes only its own record, so publication needs no cross-core coordination. A
  whole-machine figure is the sum a reader computes over the root records.
  *(Recommended: it follows from per-core root Schedulers, keeps the record layout
  unchanged, and a per-core view is what a scheduler deciding about its own core needs.)*
- **(b) Machine-wide in one record.** Core 0's root record keeps the sums of all cores; the
  other root records keep those words zero. Every core then updates core 0's record, which
  needs the kernel lock or atomics on the counters, and no per-core view exists.
- **(c) Both.** Per-core totals in each root record, plus machine-wide sums in core 0's
  record in words that are free today. Costs the cross-core updates of (b) on top of (a).
