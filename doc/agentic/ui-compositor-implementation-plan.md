# UI compositor implementation plan

Design: `ui-compositor-design.md`. Open decisions: `ui-compositor-decisions.md`.
Completed items: `ui-compositor-completed.md`. Nothing here starts until the
decisions it depends on are recorded. Input stages are in
`input-implementation-plan.md`.

## Dependencies

- Q1 and Q2 (surface memory and granularity) block C3.
- Q1 (surface memory) remains open; Q10 (input decisions) remains open.
- Answered: Q2, Q3, Q4, Q5, Q6, Q7, Q8 (`uses device` and `uses component` forms, details open), Q9, Q11, Q12 (see `ui-compositor-decisions.md`).
- Q8 (program exporters, H3): implemented in the Meddle parser, the image builder and Kickstart (2026-10-18). No import uses `via component` yet, so a compositor started as a program still needs its first `via component` import exercised by a test.
- `input-decisions.md` D1 to D4 block the real event record used by C1 and C4.
- `input-implementation-plan.md` INPUT1 supplied the event record (`libs/input-event`), and USB4 supplies the producer, for C5.

## C0: documents and interface

- Finish the three `ui-compositor-*` documents from the answers.
- Write `compositor_v1.if` as a `local` interface with a `placement` window. Edit
  in place, no version bumps.
- Exit: `meddler check` passes on the new interface and its imports.

## Done so far

- The compositor owns the input stream (parts of C1, C4 and C5): `userspace/libs/compositor-core`
  has the window list with stacking and hit-testing and the input router (pointer
  clamped to the screen, focus and raise on the first button, an implicit grab until
  the last button is released, `Enter`, `Leave` and `Focus` records, window-local
  pointer positions, keys to the focused window, release of a removed device's keys,
  resync from a snapshot), host-tested by `just test-compositor-host`.
  `userspace/gui/compositor` is the only client of the USB HID component's `input_v1`:
  it posts the buffers, folds the records into the router, recovers devices and
  dropped records with `descriptor` and `snapshot`, and traces what each routed
  window receives on the serial channel. `just test-compositor` checks it under
  QEMU, and `just boot-compositor-test` is the Raspberry Pi 3 image.
- The compositor owns the screen and serves windows (the rest of C1, C2, and C3
  without events): `userspace/libs/compositor-core` has the damage set, clipped
  premultiplied-alpha blitting, the cursor sprite and the surface size rule (host
  tests in `just test-compositor-host`). `userspace/libs/framebuffer` maps the
  framebuffer write-combining, and only the compositor does. The compositor draws a
  background, its clients' windows and the cursor in a cacheable back buffer and
  copies the damaged rectangles to the framebuffer from a frame Thread, with a
  33 ms timer that runs only while there is damage (Q7). `compositor_v1` has
  `create_window`, `commit`, `set_title`, `move_window`, `raise`, `destroy` and
  `pixel_order`; surfaces are 2 MiB Frames the compositor carves and installs in
  the client's lent table (Q1, Q2), and Kickstart gives importers of `compositor_v1`
  a surface window (`DriverInit::map_window`). The shell's `draw` makes a window and
  draws into its surface; `just test-shell` checks the lamp through it, and
  `just test-compositor` checks that the background reaches the framebuffer. The
  production image now carries the USB stack, so the compositor takes the pointer
  and keyboard there. Events reach clients (C4, see `ui-compositor-completed.md`).
  Still open: checking the cursor against injected pointer motion under QEMU,
  resizing and an overlay layer (C6), and hardware checks of the display and the
  cursor.

## C1: pure routing library

- New pure `no_std` crate `userspace/libs/compositor-core`: window list and layers, a
  bounded damage set with merge-on-overflow, clipping and blit onto slices,
  rectangle hit-testing, and the focus and implicit-grab state machine. The
  state machine produces `(target window, event)` pairs without performing I/O.
- Host tests first. They cover the record layout as far as the `input-event`
  crate defines it, damage merge, clip arithmetic, and routing sequences: press
  on A, drag across B, release on A (A keeps the grab); press with focus change;
  hover enter and leave; coalescing of motion per `Sync` frame.
- Recipe: a new focused host-test recipe in the `Justfile` (`test-compositor-host`),
  modelled on `test-object-host`. It does not exist yet and must be added as
  scoped work.
- Exit: host tests pass; no allocation on the frame path.

## C2: compositor owns the screen

- (Done.) Move the framebuffer mapping out of `userspace/shell/src/display.rs` into a
  shared library, so the compositor and nothing else maps it.
- The compositor paints a background and a cursor sprite from synthetic pointer
  motion, using the frame loop chosen by Q7.
- Exit: under QEMU raspi3b the compositor shows the background and moves a cursor
  sprite with injected motion. The shell's direct framebuffer path is removed.

## C3: surfaces and the first client

- Implement `compositor_v1` create, commit, move, raise and destroy, with surface
  Frames lent by the compositor (Q1, Q2).
- Kickstart provisions the surface window for importers of `compositor_v1`, as it
  does for `fb_v1` importers.
- The shell's `draw` creates a window; the lava thread draws into the surface and
  commits each frame.
- Exit: `just test-shell` still runs its command checks, with the lamp visible
  inside a window. The lamp report is printed from the first client's surface.

## C5: real input

- Attach the USB HID channel once USB4 is done (the event library, INPUT1, exists).
- Hardware and emulation check: a USB mouse moves the cursor and clicks focus a
  window under QEMU raspi3b, and a keyboard reaches the focused client.
- Exit: moving a mouse moves the cursor on the framebuffer.

## C6: later features

- Resize negotiation (`ResizeOffer`, new surface, swap), an overlay layer for a
  panel, and page flipping if the framebuffer offers a virtual height. Alpha
  blending is in version 1 (Q6), so it is not a C6 item.
- A separate `inputd`-style router is future work, not a stage (see the Future
  work section of `ui-compositor-decisions.md`).
- Each is its own slice with its own decision if the answer is still open.

## Verification across stages

- Host tests for `userspace/libs/compositor-core` at every stage, run through a `just`
  recipe.
- `just clippy` and `just fmt-check` for every Rust change, as the project's
  `just` workflow requires.
- Interface checks with `meddler check` for each `.if` change.
- QEMU checks through `just` recipes for C2, C3, C4 and C5. A missing recipe is a
  blocker for that stage, not a pass.
- Hardware check on RPi3 at C5.
