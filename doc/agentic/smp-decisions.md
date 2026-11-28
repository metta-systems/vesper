# SMP open decisions

Unanswered SMP questions only. Each stays open until the maintainer answers it
explicitly; recommendations are marked as such and are not decisions. An answered
question becomes a numbered section of [smp-design.md](smp-design.md#decisions) and
leaves this file; its tasks are in [smp-implementation-plan.md](smp-implementation-plan.md).

`Scheduler.replace`, non-blocking dispatch and the activation invariant across cores stay
parked in [scheduling-decisions.md](scheduling-decisions.md); per-core
dispatchers do not need them.

No SMP decision is open.

## Future work

Ideas the project might pursue later; none constrains current work. More are
listed under Later in [smp-implementation-plan.md](smp-implementation-plan.md).

- **N-SMP-1: work-stealing cores.** Let a cross-core `MoveThread` also be invoked on the
  destination Scheduler, so a core's dispatcher pulls a Thread from another core's
  Scheduler (with `WRITE` on both Schedulers, the Thread and the destination Time, as
  for a move the owner sends). Today only the owner sends (SMP-12); an experiment
  with work-stealing dispatchers would add this.
