# Scheduling open decisions

Parking lot for unanswered scheduling and Time decisions. Each entry stays open until the
maintainer answers it explicitly. Answered decisions go into
[scheduling-design.md](scheduling-design.md#decisions); their tasks into
[scheduling-implementation-plan.md](scheduling-implementation-plan.md).

## D5: scheduler hierarchy and multicore

- What is the scheduler donation schema (Phase 9)? Strict-tree ownership is decided:
  `Adopt` succeeds only on a fresh, parentless, non-root Scheduler. The other hierarchy
  rules (parent-granted allowance, exhaustion flag, `SCHED_DOMAIN_FAILED` crash
  handling) are selected: see the plan.
- `Scheduler.replace`, a non-blocking dispatch, and an activation invariant of one
  outstanding request per dispatcher across cores stay parked: per-core dispatchers
  ([SMP design](smp-design.md#decisions)) do not need them.

- **Scheduling attribution:** how should hierarchical schedulers observe and attribute
  work migrated by a protected call beyond the source Thread's own time accounting?

## D8: Time

- What happens to remaining budget when a Time is deleted?
- The `replace` register schema, rights and generation encoding (multicore, parked with
  `replace` above).

## Future work

Not decisions to make now and not planned work; revisit only if the project needs them.

- **Budget from a scheduler's own bound Time:** a Time bound to a live Thread can't give
  budget away, so a scheduler gives its children budget only from Times not bound to a
  Thread. Revisit whether this can be relaxed or reworked, for example letting the bound
  Thread itself (or its scheduler) transfer out while still refusing other key holders,
  without reopening the risk of an interrupt Thread's Time being drained.
- **Per-Thread scheduler event subscriptions:** investigate moving from the
  scheduler-level activation mask towards per-Thread subscriptions, so a scheduler can
  request activation for chosen events of chosen Threads (for example a latency-critical
  Thread's wakeup) without a hardcoded `activate_on_wake` flag. The mask stays the base
  mechanism and a subscription would act as a finer filter on it. Weigh the extra
  per-Thread state and the risk of a scheduler's data structures and the kernel's
  per-Thread configuration drifting apart.
- **Isochronous scheduler:** a nested userspace Scheduler that reserves a period and
  budget (a SCHED_ISO-like class) for Threads with periodic CPU needs. Policy lives in a
  scheduler component, as the nucleus has no periods or deadlines. Audio is the first user
  that needs it; USB isochronous pipes, and display, would use it too. HID interrupt pipes
  do not need it.
- **Kernel-published identity in `TPIDRRO_EL0`:** using the EL0 read-only,
  EL1-writable register to publish per-Thread identity to userspace, as Linux
  does for per-CPU data. Today it is always zero
  ([per-Thread architectural state](scheduling-design.md#per-thread-architectural-state)).
