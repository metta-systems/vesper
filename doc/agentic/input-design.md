# Input design

Status: design only, nothing implemented. Open decisions are listed in
`input-decisions.md`. Implementation order is in `input-implementation-plan.md`.

## Scope

One input representation that serves three uses:

- a single console, where input is bytes for a shell,
- an interactive graphical display, with a pointer, focus and grabs,
- other devices: gamepads, touch, tablets, switches.

USB HID is the first producer (see `doc/agentic/usb-design.md`). The `hid` component
currently has a placeholder `drivers/usb-hid/interfaces/input_v1.if` with
`set_leds` only; its event layout is what this document defines.

## Prior art

- **Linux evdev** ([event codes](https://lxr.missinglinkelectronics.com/linux+v5.15/Documentation/input/event-codes.rst)).
  Flat `(type, code, value)` events. `SYN_REPORT` closes a packet of simultaneous
  changes. `SYN_DROPPED` tells the client to discard up to the next report and
  re-read state. Key values: 0 release, 1 press, 2 repeat. Multitouch uses slots
  ([goldfish patch thread](https://lkml.iu.edu/1706.3/04333.html)).
  Take: flat extensible records, frame delimiters, explicit overflow and resync.
  Avoid: a Linux-specific code space, and raw events with no semantics.
- **libinput** ([architecture](https://wayland.freedesktop.org/libinput/doc/1.12.0/architecture.html)).
  Devices classified by capability. Dispatch split into touchpad, tablet, tablet
  pad, and a fallback for mice, keyboards and touchscreens. Touch comes in frames.
  Take: normalisation and policy (acceleration, tap, palm rejection, gestures)
  live above the driver, as a library.
- **xkbcommon** ([intro](https://www.xkbcommon.org/doc/current/xkb-intro.html)).
  Keymap compiled from layout names. A state object tracks modifiers, groups and
  LEDs; every key event is fed into it.
  Take: keymap and modifier state are a stage separate from the device.
- **W3C UI Events** ([key and code recommendations, 2025](https://w3.org/news/2025/ui-events-keyboardevent-key-values-and-code-values-are-w3c-recommendations)).
  `code` is the physical key position and is locale-independent. `key` is the
  layout-dependent meaning.
  Take: carry physical identity in the event; derive logical meaning later.
- **W3C Gamepad API** ([2021 working draft](https://www.w3.org/TR/2021/WD-gamepad-20210408/)).
  Only buttons (analog 0 to 1) and axes (-1 to 1), plus a `mapping` field that is
  `"standard"` or empty when the layout is unknown.
  Take: normalise to a standard layout when known, raw otherwise.
- **Fuchsia `input.report`** ([report.fidl](https://fuchsia.googlesource.com/fuchsia/+/HEAD/sdk/fidl/fuchsia.input.report/report.fidl)).
  Each device exposes a descriptor of what it can report. Reports carry optional
  per-type fields and a timestamp. Each client has a bounded FIFO that drops the
  oldest report. Output reports carry LEDs and feature state.
  Take: self-describing devices, and feedback in the same model.
- **USB HID usage tables.** `(usage page, usage)` is already a bus-neutral
  vocabulary: keyboard 0x07, LEDs 0x08, buttons 0x09, Generic Desktop 0x01 (axes,
  hats), consumer 0x0C, digitizer 0x0D. Boot keyboard reports are usage IDs, so
  decoding needs no translation table. Rust parsers exist (`hidparser`,
  `hidreport`, `hut`), not yet evaluated for use.
- **Plan 9** ([kbdfs](https://man.9front.org/8/KBDFS), [mousefs](https://plan9.io/sources/contrib/nemo/sys/man/4/mousefs)).
  A console is a text stream produced from scancodes by a translating server;
  pointer events are a separate stream and can be forwarded across machines.
  Take: the console is one adapter on top of the event stream.
- **Redox `inputd`** ([March 2025 news](https://www.redox-os.org/news/this-month-250331)).
  A multiplexer with virtual terminals between producers and consumers. No design
  document found; a pointer only.
- **Nemesis** (see `doc/agentic/usb-design.md` and `libs/io-channel`). Per-client channels
  with early demultiplex; routing to a focused client fits that pattern.

Not researched yet: Windows Raw Input, macOS IOKit HID, Android MotionEvent, W3C
Pointer Events, SDL's controller database, Wayland key repeat.

## Layers

1. **Device layer (drivers).** USB HID now; later PS/2, I2C-HID, GPIO buttons,
   virtio-input. A driver publishes a device descriptor and emits events. For USB
   boot protocol the descriptor is synthesised, and a decoder diffs consecutive
   reports into key down and up events.
2. **Event layer (the contract).** A pure `no_std` library: the fixed-size event
   record, the descriptor layout and a state tracker. The transport is the existing
   io-channel in receive mode: the client posts empty IO_Recs and the driver fills
   them with whole records.
3. **Semantics layer (libraries in the consumer or a router).** Keymap and modifier
   state, typematic repeat, pointer acceleration, gamepad standard mapping,
   gestures.
4. **Routing layer (router component).** Merges producers, owns focus, the cursor
   and grabs. Absent or trivial for a single console.

## Event record (proposal)

Flat and evdev-like, so it travels as bytes through an IO_Rec. About 24 bytes:

| Field | Meaning |
|---|---|
| `timestamp` | nanoseconds, taken in the driver at interrupt time from the time capability |
| `device` | id from the device descriptor, so one channel can carry many devices |
| `kind` | `Key`, `Rel`, `Abs`, `Sync`, `Dropped`, `DeviceAdded`, `DeviceRemoved` |
| `flags` | reserved |
| `code` | `(usage page << 16) \| usage` |
| `value` | signed 32-bit: key state 0, 1, 2; delta; absolute position |

Kinds:

- `Key`: usage plus value 0 (up), 1 (down), 2 (repeat). Mouse and gamepad buttons
  are keys on the Button page.
- `Rel`: deltas, such as X, Y and wheel.
- `Abs`: position or axis; range comes from the descriptor.
- `Sync`: closes a frame.
- `Dropped`: marks an overflow.
- `DeviceAdded` and `DeviceRemoved`: in-band hotplug, so ordering against real
  events is preserved.

Reserved for later: `Touch` (contact id), `Text` (committed characters from an
IME), `Switch` (lid, tablet mode).

## Behaviour rules

- **Frames** are never split across packets. A frame ends with `Sync`.
- **Overflow.** With no empty IO_Rec posted, the driver drops and counts, as
  `libs/serial-channel` does. The next delivery starts with `Dropped`. The consumer
  resynchronises from a state snapshot, a Call that returns the key bitmap and axis
  values through the data area, the way `SYN_DROPPED` is followed by a state read.
  Without this, a lost key-up leaves a key stuck.
- **Descriptor.** Travels through the data area as a defined `repr(C)` layout,
  because Meddle has no array or sequence type. Carries identity (bus, vendor,
  product, name), axes with usage, range and unit, button usages and LEDs.
- **Typematic repeat** is a consumer-side library driven by a timer. USB keyboards
  do not repeat in hardware, and a console and a game want different behaviour.
- **Output direction.** LEDs, rumble and haptics go as events on a second channel
  in transmit mode, rather than purpose-built Calls such as `set_leds`.
- **Trust.** Consumers validate every record (kind, code and range), as the server
  validates IO_Recs, because a driver is untrusted.

## Ends of the spectrum

**Single console.** A console adapter (library or small component) turns key
events into bytes: keymap, modifiers, Ctrl and Alt, escape sequences for cursor
keys. It presents the same byte channel the shell reads from the PL011 today, so
the shell works unchanged with a USB keyboard. Keymaps are static tables, optionally
generated at build time from xkb data, not parsed at runtime.

**Interactive graphics.** A router owns the global pointer, clamped to display
bounds, plus focus and grabs. Absolute devices (touchscreen, tablet) map into
display coordinates using descriptor ranges. A compositor can be the router.
Secure-attention and VT-switch keys are handled in the router before delivery.

**Gamepads and others.** HID gamepad usages map to a standard layout by vendor and
product, with raw fallback. Touch uses contact ids and frames. A report-descriptor
parser is needed once non-boot devices matter.

## Related

- `doc/agentic/usb-design.md`: producer side (`hid`, `usb_v1`, `usb_hcd_v1`).
- `doc/capabilities-contract.md`: PPC, xbufs and EventCounts.
- `libs/io-channel`, `libs/serial-channel`: the transport and the current byte path.
