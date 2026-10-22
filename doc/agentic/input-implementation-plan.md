# Input event implementation plan

Design: [input-design.md](input-design.md). Open decisions:
[input-decisions.md](input-decisions.md). Completed items:
[input-completed.md](input-completed.md). Nothing here starts until the decisions it
depends on are recorded.

USB HID producer work (boot reports, interrupt-IN, the `hid` component, `input_v1`)
is tracked in [usb-implementation-plan.md](usb-implementation-plan.md). Stages here
need a producer only for hardware checks.

## Dependencies

- D5 blocks INPUT2, D6 and D8 block INPUT3, and D10 blocks INPUT4. See [input-decisions.md](input-decisions.md).
- The clock is settled in [capabilities-contract.md](capabilities-contract.md): the
  monotonic physical counter in nanoseconds.
- The io-channel (`libs/io-channel`) carries the records.
- The compositor consumes the record ([ui-compositor-implementation-plan.md](ui-compositor-implementation-plan.md),
  stages C1 and C5). C1 can start on injected events. C5 needs a
  producer.

## INPUT2: overflow and replay

- Replay property tests: folding the emitted events into the state tracker reproduces
  the producer's state. Repeat with an injected `Dropped` and a resync from snapshot.
- Exit: the replay property holds for generated event sequences, including overflow.

## INPUT3: keymap and console adapter

- New `libs/keymap`: static layouts, modifier and lock state, and LED feedback events.
- The console adapter turns `Key` events into the bytes the shell reads today, on the
  same byte channel as `libs/serial-channel`.
- Host exit: tests map generated `Key` sequences to the expected byte streams.
- Hardware exit (the shell reads keystrokes from a real keyboard) needs a working USB
  producer, so it is checked after USB4 in
  [usb-implementation-plan.md](usb-implementation-plan.md).

## INPUT4: extended devices

- Gamepad standard mapping by vendor and product, with raw fallback.
- Touch with contact ids and frames (D10).
- Exit: deferred until a device is in hand.

## Moved or removed

- Boot-report decoding into events. `vesper-usb-protocol` already
  provides `for_each_change` and mouse parsing. Converting them to records is producer
  work in [usb-implementation-plan.md](usb-implementation-plan.md) USB4.
- The `input_v1` interface edit. That interface is owned by the USB plan.
- The router stage. Decided 2026-10-18: the compositor routes input.
  Pointer acceleration and focus belong to the compositor plan.

## Verification

- Host tests for each library (`libs/input-event`, `libs/keymap`).
- Replay property tests for overflow and resync (INPUT2).
- Hardware check on RPi3 at INPUT3, after USB4.
