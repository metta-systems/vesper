# UI compositor decisions

Each entry is open until the maintainer answers it. Recommendations are marked as
such; they are not decisions. Record an answer here only when the maintainer gave
it explicitly. Input decisions are in `input-decisions.md` and are referenced
here, not restated.

## Hazards

These are facts found while reading the code and the capability contract. They
constrain the decisions below.

- **H1. Revocation can reach the compositor's reads.** If a client owns surface
  memory that the compositor reads, and the client's authority is withdrawn, the
  next compositor read of that mapping faults. The contract's expected policy is
  Thread termination (`capabilities-contract.md`, D1 follow-up). A hostile client
  could therefore take the compositor down, unless the compositor owns the surface
  memory itself (Q1). Selective Revoke is not enabled.
- **H2. Frame sizes and table size.** Frames come in 4 KiB, 2 MiB and 1 GiB. A
  1024x768 surface of 32-bit pixels is 3 MiB: 768 4 KiB Frames, or two 2 MiB
  Frames plus a 1 MiB tail. The shell's lent table has 1024 slots
  (`FRAME_TABLE_BITS = 10`), and driver tables have 256. Many surfaces at once
  will exhaust a table of 4 KiB Frames quickly.
- **H3. Program exporters are not wired.** Kickstart resolves imports through
  device-tree links (`uses X via ...`). A program (`[[component]]` with
  `program = true`) has no node, so nothing can link a client to a program that
  exports an interface. Q8 covers the choice.
- **H4. Input source.** `drivers/usb-hid` publishes boot keyboard and mouse events over
  the `input_v1` io-channel (record in `input-design.md`; `input-decisions.md` has no
  open event-record items), with `descriptor` and `snapshot` calls. It is checked in
  QEMU with injected keys and not yet on a Raspberry Pi; the event library is
  `libs/input-event` (INPUT1).
- **H5. Framebuffer hand-off is one-shot.** `fb_v1.frames` gives the framebuffer's
  memory away for good, and a second call fails. Whatever holds the framebuffer
  holds it until reset, so the compositor must be the only holder once it exists.
- **H6. Write-combining is a ceiling, not a guarantee.** The framebuffer is mapped
  write-combining, and the video memory's ceiling allows it. Surfaces in RAM are
  cacheable, so copying damaged rectangles to the framebuffer must go through the
  write-combining mapping, and the compositor must not read the framebuffer back
  for hit-testing.
- **H7. Reading client pixels for hit-testing.** Yutani hit-tests by reading each
  window's pixels, to honour alpha. The compositor should use window rectangles
  instead (Q6 sets whether alpha ever matters). Reading client memory on the input
  path is a hazard by itself, because it couples input latency to client behavior.

## Q1. Who owns surface memory

- **A. The compositor carves surface Frames from its own Untyped and installs them
  into the client's lent table**, as `fb_v1.frames` does (recommended). The
  compositor keeps the authority, so H1 does not apply to the compositor's own
  reads. Memory is accounted to the compositor, and a per-client quota is
  compositor policy.
- **B. The client carves surfaces from its own Untyped and lends a table of Frames
  to the compositor.** Memory is accounted to the client, but H1 applies.

Answer (2026-10-19): A, the compositor owns the surface Frames and the application
just draws there.

## Q2. Surface granularity

- **A. Frames are 4 KiB only.** Simple, many slots (H2).
- **B. One 2 MiB Frame per surface where possible, 4 KiB Frames for the tail.**
  Fewer slots, but the surface's size and address must be 2 MiB aligned where they
  matter.
- **C. Choose by size.** Most flexible, most code.

Recommendation: B for version 1, because it keeps slot counts under control.

Answer (2026-10-18): C, granularity chosen by surface size.

Rule (2026-10-19): a surface's memory is its size rounded up to a multiple of 2 MiB,
made of 2 MiB Frames. A 3 MiB surface gets 4 MiB, two Frames. The slack
accommodates small size changes, which change only the stride, without
reallocating.

## Q3. Event channel granularity

- **A. One io-channel per client, with the window id in each record**
  (recommended; matches `input-decisions.md` D4).
- **B. One io-channel per window.** More isolation, more channels to manage.

Answer (2026-10-18): A, one io-channel per client, with the window id in each record.

## Q4. Where routing lives

- **A. The compositor routes input itself** (recommended while it is the only
  consumer; `input-decisions.md` D9 option 3).
- **B. A separate `inputd`-style router first** (`input-decisions.md` D9 option 2).

Answer (2026-10-18): A for now, the compositor routes input itself. A separate
`inputd`-style router is future work (see the Future work section). This answers
the compositor side of `input-decisions.md` D9; the input record for D9 is still to
be entered there.

## Q5. Decorations and interactive move

- **A. Client-side decoration library; the compositor runs the grab for
  Alt-drag** (recommended; Wayland style).
- **B. The compositor draws frames and title bars.** Consistent, but the
  compositor grows policy and drawing code.

Answer (2026-10-18): A, client-side decoration library. The compositor runs the
grab for Alt-drag, as option A states.

## Q6. Pixel format and alpha in version 1

- **A. Opaque, native pixel order only** (recommended). Hit-testing by rectangle
  (H7).
- **B. ARGB with premultiplied blending from the start.** Needed for panels and
  shadows; costs blend time on a software compositor.

Answer (2026-10-18): B, ARGB with premultiplied blending from version 1.
Surfaces carry alpha, so the compositor blends them. Hit-testing still uses window
rectangles unless a later decision says otherwise.

## Q7. Redraw trigger

- **A. A fixed frame timer, about 16 to 33 ms** (as the lava lamp does with
  `FRAME_NS`).
- **B. Wake only when damage exists**, using an EventCount that clients advance on
  commit (recommended for an idle desktop).
- **C. Timer that runs only while damage exists**, a hybrid of A and B.

Answer (2026-10-18): C, a timer driven by an adaptive target frame rate, running
only while damage is present.

Rule (2026-10-19): A for now, a fixed 33 ms timer that runs only while damage
exists. The adaptation rule is deferred.

## Q8. Wiring a program that exports an interface (H3)

- **A. Extend `uses` with a program target**, so a program can be an exporter.
- **B. Give the compositor a device-tree-like pseudo-node** that Kickstart knows.
- **C. A different composition form**, to be proposed.

Answer (2026-10-18): extend the `uses` clause with two forms, selected by a keyword after `via`:

- `uses mailbox_v1 via device firmware, mboxes { property }`: a device-tree link, as today's `uses mailbox_v1 via firmware, mboxes { property }` but with the `device` keyword.
- `uses fb_v1 via component fb`: a component variant, naming the component directly.

Implemented (2026-10-18): the parser accepts only the `device` and `component` forms, and the old spellings without `device` are rejected. `via device compatible "..."` is the compatible-string form. `via component fb` names the manifest component; its one loaded instance exporting the interface is the exporter, none leaves the import not granted, and more than one is reported and not granted. The contract and all existing `imports.if` files use the new spelling. A program can be named as the exporter through `via component`, but no import in the tree uses `via component` yet, so this path is not exercised.

## Q9. Where the component lives

- **A. `userspace/compositor`**, a program like the shell (recommended, given H3
  and that it needs no device node).
- **B. `drivers/compositor`**, next to `fb`.

Answer (2026-10-18): `userspace/gui/compositor`.

## Q10. Input decisions needed first

Which of the following are answered before the compositor's routing code is
written, because the event record is shared by both channels:

- `input-decisions.md` D1 (event code namespace), recommended HID usages.
- D2 (record shape), recommended flat record.
- D3 (frame marking), recommended explicit `Sync`.
- D4 (channel granularity), recommended one channel per producer.
- D9 (router), see Q4.

Status: D1 to D4 answered 2026-10-18 and recorded in `input-design.md`, with D13 (the
`subject` field). Stages C1 (pure routing library) can start now, on the event library `libs/input-event`,
which defines the record type; see
the implementation plan.

## Q11. Overflow policy for client event channels

- **A. Drop newest, emit `Dropped`, resync from state**, as `input-decisions.md`
  D5 recommends.
- **B. Drop oldest.** Keeps recent state but silently loses events.

Answer (2026-10-18): A, drop newest, emit `Dropped`, and resync from state, as
recommended. This applies to the compositor's client event channels. It is input
related, so `input-decisions.md` D5 for the input channel itself is still to be
entered there.

## Future work

- **Separate `inputd`-style router.** A multiplexer between input producers and
  the compositor, as Redox `inputd` does, for when more than one consumer needs
  input. Not planned for version 1 (Q4).
