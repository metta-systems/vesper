# Scheduling open decisions

Unanswered scheduling and Time questions only. Each entry stays open until the
maintainer answers it explicitly. An answered question becomes a numbered section of
[scheduling-design.md](scheduling-design.md#decisions) and leaves this file; its tasks
go into [scheduling-implementation-plan.md](scheduling-implementation-plan.md).

## Q-SCHEDULING-1: the scheduler donation schema

What is the scheduler donation schema? Strict-tree ownership is decided: `Adopt`
succeeds only on a fresh, parentless, non-root Scheduler (SCHEDULING-18). The other
hierarchy rules (parent-granted allowance, exhaustion flag, `SCHED_DOMAIN_FAILED` crash
handling) are selected: see P-SCHEDULING-9.

## Q-SCHEDULING-2: scheduling attribution across protected calls

How should hierarchical schedulers observe and attribute work migrated by a protected
call beyond the source Thread's own time accounting?

## Q-SCHEDULING-3: remaining budget when a Time is deleted

What happens to remaining budget when a Time is deleted?

## Q-SCHEDULING-4: replace and non-blocking dispatch

`Scheduler.replace`, its register schema, rights and generation encoding, a non-blocking
dispatch, and an activation invariant of one outstanding request per dispatcher across
cores stay parked: per-core dispatchers ([SMP design](smp-design.md#decisions)) do not
need them. The tasks wait in P-SCHEDULING-14.

## Future work

Not decisions to make now and not planned work; revisit only if the project needs them.

- **N-SCHEDULING-1: budget from a scheduler's own bound Time.** A Time bound to a live
  Thread can't give budget away (SCHEDULING-31), so a scheduler gives its children
  budget only from Times not bound to a Thread. Revisit whether this can be relaxed or
  reworked, for example letting the bound Thread itself (or its scheduler) transfer out
  while still refusing other key holders, without reopening the risk of an interrupt
  Thread's Time being drained.
- **N-SCHEDULING-2: per-Thread scheduler event subscriptions.** Investigate moving from
  the scheduler-level activation mask towards per-Thread subscriptions, so a scheduler
  can request activation for chosen events of chosen Threads (for example a
  latency-critical Thread's wakeup) without a hardcoded `activate_on_wake` flag. The
  mask stays the base mechanism and a subscription would act as a finer filter on it.
  Weigh the extra per-Thread state and the risk of a scheduler's data structures and
  the kernel's per-Thread configuration drifting apart.
- **N-SCHEDULING-3: an isochronous scheduler.** A nested userspace Scheduler that
  reserves a period and budget (a SCHED_ISO-like class) for Threads with periodic CPU
  needs. Policy lives in a scheduler component, as the nucleus has no periods or
  deadlines. Audio is the first user that needs it; USB isochronous pipes, and display,
  would use it too. HID interrupt pipes do not need it.
- **N-SCHEDULING-4: kernel-published identity in `TPIDRRO_EL0`.** Using the EL0
  read-only, EL1-writable register to publish per-Thread identity to userspace, as
  Linux does for per-CPU data. Today it is always zero (SCHEDULING-34).
