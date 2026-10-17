# Input decisions

Each entry is open until the maintainer answers it. Recommendations are marked as
such; they are not decisions. Record an answer here only when the maintainer gave
it explicitly.

## D1. Event code namespace

Options:

- **HID `(usage page, usage)`** (recommended). Bus-neutral, boot decoding needs no
  translation, covers keyboard, buttons, axes, consumer and LEDs.
- evdev codes. Familiar, but Linux-specific.
- A custom enum. Full control, but a new vocabulary to maintain.

Status: open.

## D2. Record shape

Options:

- **Flat evdev-like record** (recommended). One fixed-size type; trivially travels
  through an IO_Rec.
- Typed reports per device kind, as Fuchsia does. Richer, but a variable schema is
  harder to transport without arrays.

Status: open.

## D3. Frame marking

Options:

- **Explicit `Sync` event** (recommended). Simple to consume, matches evdev.
- A "more in frame" flag on each event. Saves one record, but a lost record breaks
  framing silently.

Status: open.

## D4. Channel granularity

Options:

- **One channel per producer component, device id in each record** (recommended).
- One channel per device. More isolation, more channels to manage.

Status: open.

## D5. Overflow policy

Options:

- **Drop newest, emit `Dropped`, resynchronise from a state snapshot** (recommended).
  Keeps the driver non-blocking and makes loss visible.
- Drop oldest, as Fuchsia does. Keeps the most recent state, but loses events
  silently unless the consumer compares sequence numbers.

Status: open.

## D6. Repeat and keymap placement

Options:

- **Consumer-side libraries** (recommended). Repeat and layout differ per consumer,
  and the driver stays a dumb decoder.
- In the `hid` driver. Simpler for one consumer, but it duplicates policy per
  driver.

Status: open.

## D7. Output direction (LEDs, rumble)

Options:

- **Events on a transmit channel** (recommended). One mechanism for every output,
  and it matches the io-channel design.
- Purpose-built Calls such as `set_leds`. Simple now, but one interface per output
  kind.

Status: open. The current placeholder `input_v1.if` uses `set_leds`, so it changes
if D7 is answered for events.

## D8. Console path

Options:

- **Adapter presenting a serial-compatible byte channel** (recommended). The shell
  stays unchanged.
- The shell consumes events directly. Richer, but the shell takes on keymap work.

Status: open.

## D9. Router

Options:

- **None in v1** (recommended while there is one consumer).
- A separate `inputd`-style router, as Redox has.
- The compositor performs routing itself.

Status: open. Needed before I4.

## D10. Touch representation

Options:

- evdev-style slots.
- Explicit `Touch` records with contact ids (leaning this way, but unanswered).

Status: open, deferred until a touch device exists (I5).

## D11. Timestamp source

Options:

- **Time capability, read in the driver at interrupt time** (recommended, matching
  the time capability design).
- Consumer-side timestamps. Simpler, but they include scheduling latency.

Status: open. Confirm against `doc/capabilities-time.md`.
