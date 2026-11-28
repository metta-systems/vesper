# Input event design

The numbered sections are the maintainer's decisions; the sections after them
describe the mechanism. Open questions are in
[input-decisions.md](input-decisions.md), open tasks in
[input-implementation-plan.md](input-implementation-plan.md), finished ones in
[input-completed.md](input-completed.md). The event library, `libs/input-event`,
is built.

The input event contract: the record that carries keyboard, pointer and other device
events, how it travels, how frames and overflow are handled, and what consumers do
with it. Producers and their device protocols are out of scope. USB HID is described
in [usb-design.md](usb-design.md).

Consumers in scope:

- a console, where key events become bytes for a shell,
- a graphical compositor, which routes events to windows
  ([ui-compositor-design.md](ui-compositor-design.md)).

## INPUT-1: a flat event record

2026-10-18. Events travel as one flat, evdev-like 24-byte record, with frames,
overflow and hotplug as kinds of the same record (see Event record).

## INPUT-2: HID usages as the code namespace

2026-10-18. A record's `code` is `(usage page << 16) | usage`, from the USB HID
usage tables. A keyboard code is the raw HID usage; the consumer derives the
logical key.

## INPUT-3: frames end with `Sync`

2026-10-18. Every delivery ends with a `Sync`, even a single event. A frame is
never split across packets, and one frame can span devices of the channel.

## INPUT-4: one channel per producer

2026-10-18. One channel per producer component, with the device id in each
record's `subject`.

## INPUT-5: output on a transmit channel

2026-10-18. Output events (LEDs, rumble) use a transmit channel with the same
record format. There is no interim Call.

## INPUT-6: timestamps at the device interrupt

2026-10-18. A record's timestamp is read from the physical counter, in
nanoseconds, when the producer handles the device interrupt.

## INPUT-7: hotplug in band, ids never reused

2026-10-18. `DeviceAdded` and `DeviceRemoved` are records in the stream, so
their order against real events is kept. A device id is unique within the
channel's life and never reused.

## INPUT-8: the console is an adapter

2026-10-29. The console is an adapter that presents a serial-compatible byte
channel to the shell. Not chosen: the shell consuming events directly.

## INPUT-9: the compositor routes input

2026-10-18, with UI-COMPOSITOR-4. Routing (focus, pointer, grabs) belongs to
the compositor; a separate `inputd`-style router is future work
(N-UI-COMPOSITOR-1).

## Prior art

- **Linux evdev** ([event codes](https://lxr.missinglinkelectronics.com/linux+v5.15/Documentation/input/event-codes.rst)).
  Flat `(type, code, value)` events. `SYN_REPORT` closes a packet of simultaneous
  changes. `SYN_DROPPED` tells the client to discard up to the next report and
  re-read state. Multitouch uses slots.
  Take: flat extensible records, frame delimiters, explicit overflow and resync.
  Avoid: a Linux-specific code space.
- **libinput** ([architecture](https://wayland.freedesktop.org/libinput/doc/1.12.0/architecture.html)).
  Normalisation and policy (acceleration, tap, palm rejection, gestures) live above
  the producer, as a library.
- **xkbcommon** ([intro](https://www.xkbcommon.org/doc/current/xkb-intro.html)).
  Keymap and modifier state are a stage separate from the device.
- **W3C UI Events** ([key and code recommendations, 2025](https://w3.org/news/2025/ui-events-keyboardevent-key-values-and-code-values-are-w3c-recommendations)).
  `code` is the physical key position and is locale-independent. `key` is the
  layout-dependent meaning.
  Take: carry physical identity in the event; derive logical meaning later.
- **W3C Gamepad API** ([2021 working draft](https://www.w3.org/TR/2021/WD-gamepad-20210408/)).
  Buttons and axes, with a `mapping` field that is `"standard"` or empty when the
  layout is unknown.
  Take: normalise to a standard layout when known, raw otherwise.
- **Fuchsia `input.report`** ([report.fidl](https://fuchsia.googlesource.com/fuchsia/+/HEAD/sdk/fidl/fuchsia.input.report/report.fidl)).
  Each device exposes a descriptor of what it can report. Each client has a bounded
  FIFO that drops the oldest report.
  Take: self-describing devices.
- **USB HID usage tables.** `(usage page, usage)` is a bus-neutral vocabulary:
  keyboard 0x07, LEDs 0x08, buttons 0x09, Generic Desktop 0x01 (axes, hats),
  consumer 0x0C, digitizer 0x0D.
- **Plan 9** ([kbdfs](https://man.9front.org/8/KBDFS), [mousefs](https://plan9.io/sources/contrib/nemo/sys/man/4/mousefs)).
  The console is a text stream produced from key events by a translating server.
  Take: the console is one adapter on top of the event stream.
- **Nemesis** ([usb-design.md](usb-design.md), `libs/io-channel`). Per-client channels
  with early demultiplex.

Not researched yet: Windows Raw Input, macOS IOKit HID, Android MotionEvent, W3C
Pointer Events, SDL's controller database, Wayland key repeat.

## Layers

1. **Event layer (the contract).** A pure `no_std` library: the fixed-size record,
   the descriptor layout and a state tracker. Transport is an io-channel in receive
   mode: the consumer posts empty IO_Recs and the producer fills them with whole
   records.
2. **Consumer semantics (libraries in each consumer).** Keymap and modifier state,
   typematic repeat, gamepad standard mapping, gestures.

Routing (focus, pointer, grabs) belongs to the compositor and is not part of this
layer (INPUT-9, [ui-compositor-design.md](ui-compositor-design.md)).

## Event record

Flat and evdev-like (INPUT-1), so it travels as bytes through an IO_Rec. 24
bytes, 8-byte aligned, little-endian:

| Offset | Field | Type | Meaning |
|---:|---|---|---|
| 0 | `timestamp` | `u64` | nanoseconds from the physical counter, read by the producer when it handles the device interrupt |
| 8 | `subject` | `u32` | what the record is about: on producer records the device, by the id from its descriptor, so one channel can carry many devices; on compositor-to-client records the window |
| 12 | `code` | `u32` | `(usage page << 16) \| usage` |
| 16 | `value` | `i32` | key state 0, 1, 2; delta; absolute position |
| 20 | `kind` | `u8` | see below |
| 21 | `flags` | `u8` | reserved, zero |
| 22 | (padding) | `u16` | reserved, zero |

A consumer rejects and counts a record with an unknown `kind` (producers are
untrusted).

Kinds (the wire number in parentheses; zero is never a kind):

- `Key` (1): usage plus value 0 (up), 1 (down), 2 (repeat). Mouse and gamepad buttons
  are keys on the Button page. Modifier keys (usages 0xE0 to 0xE7) are keys like
  any other: a record per physical press and release, and nothing else. A producer sends only changes, never the full set of held keys, and never carries modifier state in `flags`; the consumer tracks which modifiers are active. A producer emits value 0
  and 1 only. Value 2 is synthesised by a consumer's repeat library, and is part of
  the contract only for records a consumer forwards.
- `Rel` (2): deltas, one axis per record, such as X, Y and wheel.
- `Abs` (3): position or axis, one axis per record; range comes from the descriptor.
- `Sync` (4): closes a frame. Every delivery ends with one, even a single event, and one
  frame can span devices of the channel.
- `Dropped` (5): marks an overflow.
- `DeviceAdded` (6) and `DeviceRemoved` (7): in-band hotplug, so ordering against real
  events is preserved.
- `Focus` (8), `Enter` (9), `Leave` (10), `ResizeOffer` (11), `CloseRequest` (12): window notifications the
  compositor sends to its clients ([ui-compositor-design.md](ui-compositor-design.md)).
  Producers never emit them.

Reserved for later: `Touch` (contact id), `Text` (committed characters from an
IME), `Switch` (lid, tablet mode).

Codes use HID usages (INPUT-2) on these pages in v1: Generic Desktop 0x01, Keyboard 0x07,
LED 0x08, Button 0x09, Consumer 0x0C and Digitizer 0x0D. A keyboard `code` is the raw
HID usage; the consumer derives the logical key from it.

**Compositor-to-client records.** The compositor rewrites pointer coordinates into
window-local space and puts the window they belong to in `subject`. Its own records
(`Focus`, `Enter`, `Leave`, `ResizeOffer`, `CloseRequest`) name their window there too.
A client never sees the source device of a forwarded event.

## Descriptor

Travels through the data area as a fixed 272-byte layout, little-endian, because Meddle
has no array or sequence type. It carries identity (bus, vendor, product, version,
name), axes with usage, range, resolution and unit, and the keys the device has as
ranges of usages.

| Offset | Field | Type |
|---:|---|---|
| 0 | layout version | `u32`, 1 |
| 4 | `bus` | `u16` (Linux numbers: USB 3, virtual 6) |
| 6 | `vendor` | `u16` |
| 8 | `product` | `u16` |
| 10 | `version` | `u16` |
| 12 | name length | `u8`, at most 32 |
| 13 | axis count | `u8`, at most 8 |
| 14 | key range count | `u8`, at most 8 |
| 15 | reserved | `u8`, zero |
| 16 | name | 32 bytes, zero after the length |
| 48 | axes | 8 x 20 bytes: `code` `u32`, `min` `i32`, `max` `i32`, `resolution` `u32`, `unit` `u8` (0 none, 1 millimetre, 2 degree), 3 zero bytes |
| 208 | key ranges | 8 x 8 bytes: `page` `u16`, `first` `u16`, `last` `u16`, 2 zero bytes |

Slots past a count are zero, and a consumer refuses a descriptor that breaks any of
these rules. The layout and a state tracker (key and button bitmaps, absolute axes,
the stale mark after `Dropped`, release on `DeviceRemoved`) are in `libs/input-event`
(`vesper-input-event`). A producer exposes the descriptor and the state snapshot as
Calls on its interface, `input_v1`, through its `descriptor` and `snapshot`
functions ([usb-design.md](usb-design.md)).

## Behaviour rules

- **Frames** are never split across packets. A frame ends with `Sync` (INPUT-3).
- **Channels.** One channel per producer component, with the device id in each
  record (in `subject`, INPUT-4). A producer assigns an id when it emits `DeviceAdded`; the id is unique within
  the channel's life and never reused (INPUT-7), so ids do not survive unplugging a device. A consumer that wants to recognise a returning device matches the identity in its descriptor.
- **Removal.** Because producers send only changes, a consumer treats every key and
  button of a device as released when it receives `DeviceRemoved` for it.
- **Overflow.** When no empty IO_Rec is posted, the producer drops and counts, as
  `libs/serial-channel` does. The next delivery starts with `Dropped`. The consumer
  resynchronises from a state snapshot, a Call that returns the key bitmap and axis
  values through the data area, as `SYN_DROPPED` is followed by a state read.
  Without this, a lost key-up leaves a key stuck. The policy itself is
  Q-INPUT-1, still open.
- **Timestamps** (INPUT-6) are read from the clock when the producer handles the device
  interrupt. The host controller completes transfers by interrupt, so a producer's
  interrupt Thread reads the clock when it services a report's interrupt, with no
  polling interval in the timestamp.
- **Typematic repeat** is a consumer-side library driven by a timer. Producers do not
  repeat in hardware, and a console and a game want different behaviour.
- **Output direction (INPUT-5).** Output events (LEDs, rumble) use a transmit channel with the
  same record format. There is no interim Call; LED output waits for the transmit
  channel.
- **Trust.** Consumers validate every record (kind, code and range) because a
  producer is untrusted.

## Consumers

**Console.** An adapter turns `Key` events into bytes: keymap, modifiers, Ctrl and
Alt, escape sequences for cursor keys. It presents the byte channel the shell reads
today, so the shell is unchanged. Keymaps are static tables, optionally generated at
build time from xkb data, not parsed at runtime (INPUT-8). Serial input joins the same path: the log server owns
the PL011 and puts received bytes into the input events (logging decision L11 in
[logging-design.md](logging-design.md)), and the adapter turns them back into the
shell's bytes.

**Compositor.** Consumes the same record and routes it (see
[ui-compositor-design.md](ui-compositor-design.md)). Pointer acceleration and focus
are compositor policy.

**Gamepads and touch.** Standard mapping by vendor and product, with raw fallback.
Touch uses contact ids and frames. Deferred until a device is in hand.

## Future work

- **N-INPUT-1: strongly typed payloads.** Look at Fuchsia's typed per-device reports
  (`fuchsia.input.report`) as an alternative to the flat record, once the flat record's
  limits show.
- **N-INPUT-2: a separate window-event record.** Investigate a separate record type with a shared
  header for compositor window notifications, instead of adding their kinds to the
  input `kind` enum.
- **N-INPUT-3: high-rate pointer devices.** Investigate channel load from high-DPI mice (for
  example 1500 dpi), which may flood the channels with `Rel` records: coalescing, a
  single two-axis record, or rate limits.

## Related

- [input-decisions.md](input-decisions.md): open event decisions.
- [usb-design.md](usb-design.md): producer side (USB HID).
- [capabilities-contract.md](capabilities-contract.md): io-channel, EventCounts and
  the monotonic clock.
- `libs/io-channel`, `libs/serial-channel`: the transport and the current byte path.
