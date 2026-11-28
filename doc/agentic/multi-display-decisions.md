# Multi-display decisions

Open questions only. Answered ones are recorded in the Decisions section of
[multi-display-design.md](multi-display-design.md); the tasks waiting on them
are in [multi-display-implementation-plan.md](multi-display-implementation-plan.md).
Q-MULTI-DISPLAY-1 and Q-MULTI-DISPLAY-2 will be discussed in more detail before
work resumes.

## Q-MULTI-DISPLAY-1: how a provider's change reaches the compositor

When an output's presence or mode changes, its provider bumps the generation
that `describe` returns. That much is settled (MULTI-DISPLAY-9: EventCount +
generation).
The open part is how the compositor learns that it should call `describe`
again.

- **(a) A channel on `display_v1`.** Kickstart already gives each pair of an
  interface that declares `channel N;` an io-channel, with an EventCount the
  exporter advances and the client awaits. `input_v1` reaches the compositor
  this way.
  - For: no new mechanism. The channel could also carry a change record.
  - Against:
    - a control page per pair (plus data pages if records travel) out of
      the 2 MiB data window that all pairs share;
    - the compositor would have to wait on several EventCounts at once (one
      per output, plus input), which means a WaitSet or a Thread per output.
- **(b) The compositor polls `describe`** on a slow timer (say once a
  second), from the frame Thread or a Thread of its own.
  - For: nothing new in Kickstart, Meddle or the providers.
  - Against:
    - a wakeup and a call per output per period, even when nothing changes;
    - arrival or removal seen up to a period late.
- **(c) A `display_v1` call that installs an await-only key** to the
  provider's EventCount in the caller's table, the way `display_layout`
  installs Invocation keys.
  - For: one EventCount per output, which a WaitSet can watch; no data
    window cost.
  - Against: a new pattern. `Masters` installs Invocation keys only, so
    installing another kind of key (an EventCount with only the right to
    await) needs a helper of its own. Q-MULTI-DISPLAY-2 (a) would use the
    same helper.

Whichever is chosen, fb also needs its own source of change: polling the
firmware for the display's presence and EDID.

## Q-MULTI-DISPLAY-2: how layout readers learn of a change

The compositor bumps `display_layout_v1.generation` on every move or turn,
and will on every arrival and removal. The open part is how a reader such as
a panel, a settings tool or a client that keeps its windows on one output
knows that it should read the layout again.

- **(a) `display_layout` also installs an await-only key** to a compositor
  EventCount that is advanced on every change. It would go in the slot after
  the interface's keys.
  - For: readers wait without polling, and a WaitSet can combine it with
    their other EventCounts.
  - Against: the same new key-installing helper as Q-MULTI-DISPLAY-1 (c), and one more slot
    and wire key in `display_layout`'s answer.
- **(b) A `LayoutChanged` record on the client's input io-channel**, which
  every compositor client already holds and reads.
  - For: no new mechanism on the kernel side; one new record kind in
    `vesper-input-event`.
  - Against:
    - layout news mixes with window events;
    - a client that has no buffer posted loses the record. It would then
      see `Dropped`, and must compare `generation` when it resynchronises;
    - only clients with an event channel can learn of changes.
- **(c) Readers poll `generation`.**
  - For: nothing new.
  - Against: wakeups and calls for nothing, and changes seen late.

If Q-MULTI-DISPLAY-1 and Q-MULTI-DISPLAY-2 both pick "install an await-only
key" ((c) and (a)), they share one helper.
