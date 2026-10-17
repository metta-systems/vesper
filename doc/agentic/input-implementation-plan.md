# Input implementation plan

Design: `input-design.md`. Open decisions: `input-decisions.md`. Nothing here
starts until the decisions it depends on are recorded.

## Dependencies

- `input-decisions.md` D1 to D4 block I0 and I1.
- USB HID (`doc/agentic/usb-design.md`, P1 to P4) supplies the first real producer.
- The shell's byte path (`userspace/shell`) is the consumer for I3.

## I0: interface and document

- Write the decided parts of `input-design.md` into final form.
- Replace the placeholder `drivers/usb-hid/interfaces/input_v1.if` with the decided
  control calls: descriptor fetch and state snapshot. Prototype interfaces are edited
  in place, with no version bumps.
- Exit: `meddler check` passes on the interface and its imports.

## I1: event library

- New pure `no_std` crate `libs/input-event`: record type, descriptor layout, usage
  constants for the pages in use (keyboard, LEDs, buttons, Generic Desktop, consumer),
  and a state tracker that folds events into key and axis state.
- Host-testable. Tests assert the wire layout (size, field offsets, the code packing)
  so the encoding cannot drift silently.
- Exit: tests pass on the host; the record is exactly as the decided layout.

## I2: boot decoders

- In the `hid` component's library: boot keyboard and boot mouse report decoders.
  Decoding is a diff of consecutive reports into `Key`, `Rel` and `Sync` events.
- Property test: replaying the emitted events into the state tracker reproduces the
  last report. Repeat with an injected `Dropped` and resync from snapshot.
- Exit: decoder tests pass; the replay property holds for generated report sequences.

## I3: keymap and console adapter

- New `libs/keymap`: static layouts, modifier and lock state, LED feedback.
- Console adapter turns `Key` events into the bytes the shell reads today.
- Exit: the shell reads keystrokes from a USB keyboard under QEMU or on RPi3
  hardware, with the PL011 path unchanged.

## I4: router and pointer policy

- Router component (or compositor module) owning the pointer position, clamping,
  focus and grabs. Pointer acceleration as a library.
- Needed once the graphical shell wants a mouse.
- Exit: moving a USB mouse moves the cursor on the framebuffer.

## I5: extended devices

- Gamepad standard mapping by vendor and product, with raw fallback.
- HID report-descriptor parsing for non-boot devices (crate choice undecided).
- Touch with contact ids and frames.
- Exit: deferred until a device is in hand.

## Verification across stages

- Host tests for every library (`libs/input-event`, `libs/keymap`, decoders).
- Replay property tests for overflow and resync.
- Interface checks with `meddler check` for each `.if` change.
- Hardware check on RPi3 at I3 and I4.
