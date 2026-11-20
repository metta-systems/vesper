# Input event decisions

Parking lot for unanswered input event decisions. This is the only place input
decisions are tracked; other documents link here. Each entry stays open until the
maintainer answers it. Recommendations are marked as such; they are not decisions.
Record an answer here only when the maintainer gave it explicitly. Answered items
move to [input-completed.md](input-completed.md), section Completed decisions, once the
design and plan reflect them.

Scope: the event record, its transport and its consumers. USB HID producer
decisions (report decoding, descriptor synthesis, output reports, device-tree
links) are in [usb-decisions.md](usb-decisions.md).

Answered decisions are recorded in [input-design.md](input-design.md) and listed in
[input-completed.md](input-completed.md). The questions below are what remains.

## D5. Overflow policy

Options:

- **Drop newest, emit `Dropped`, resynchronise from a state snapshot** (recommended).
  Keeps the producer non-blocking and makes loss visible.
- Drop oldest, as Fuchsia does. Keeps the most recent state, but loses events
  silently unless the consumer compares sequence numbers.

Status: open. Blocks INPUT2.

## D6. Repeat and keymap placement

Options:

- **Consumer-side libraries** (recommended). Repeat and layout differ per consumer,
  and the producer stays a dumb decoder.
- In the producer driver. Simpler for one consumer, but it duplicates policy per
  driver.

Status: open. Blocks INPUT3.

## D10. Touch representation

Options:

- evdev-style slots.
- Explicit `Touch` records with contact ids (leaning this way, but unanswered).

Status: open, deferred until a touch device exists (INPUT4).
