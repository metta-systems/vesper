# Input event decisions

Open input event questions only. Each stays open until the maintainer answers
it; recommendations are marked as such and are not decisions. An answered
question becomes a numbered section of [input-design.md](input-design.md) and
leaves this file.

Scope: the event record, its transport and its consumers. USB HID producer
decisions (report decoding, descriptor synthesis, output reports, device-tree
links) are in [usb-decisions.md](usb-decisions.md).

## Q-INPUT-1: overflow policy

Options:

- **Drop newest, emit `Dropped`, resynchronise from a state snapshot** (recommended).
  Keeps the producer non-blocking and makes loss visible.
- Drop oldest, as Fuchsia does. Keeps the most recent state, but loses events
  silently unless the consumer compares sequence numbers.

Status: open. Blocks P-INPUT-2.

## Q-INPUT-2: repeat and keymap placement

Options:

- **Consumer-side libraries** (recommended). Repeat and layout differ per consumer,
  and the producer stays a dumb decoder.
- In the producer driver. Simpler for one consumer, but it duplicates policy per
  driver.

Status: open. Blocks P-INPUT-3.

## Q-INPUT-3: touch representation

Options:

- evdev-style slots.
- Explicit `Touch` records with contact ids (leaning this way, but unanswered).

Status: open, deferred until a touch device exists (P-INPUT-4).
