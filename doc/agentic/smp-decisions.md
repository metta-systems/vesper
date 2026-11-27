# SMP open decisions

Parking lot for unanswered SMP decisions. Each entry stays open until the maintainer
answers it explicitly; recommendations are marked as such and are not decisions. Answered
decisions are in [smp-design.md](smp-design.md#decisions); their tasks are in
[smp-implementation-plan.md](smp-implementation-plan.md).

`Scheduler.replace`, non-blocking dispatch and the activation invariant across cores stay
parked in [scheduling-decisions.md](scheduling-decisions.md); per-core
dispatchers do not need them.

No SMP decision is open.

## Future work

Ideas the project might pursue later; none constrains current work.

- **Work-stealing cores.** Let a cross-core `MoveThread` also be invoked on the
  destination Scheduler, so a core's dispatcher pulls a Thread from another core's
  Scheduler (with `WRITE` on both Schedulers, the Thread and the destination Time, as
  for a move the owner sends). Today only the owner sends (C4.3.1(a)); an experiment
  with work-stealing dispatchers would add this.
