# UI compositor implementation plan

Design: `ui-compositor-design.md`. Completed items and the answered decisions:
`ui-compositor-completed.md`. Nothing here starts until the decisions it depends on
are recorded. Input stages are in `input-implementation-plan.md`; multiple outputs
are in `multi-display-implementation-plan.md`.

## Dependencies

- Q1 to Q12 are answered (`ui-compositor-completed.md`); no compositor decision is
  open.
- Input decisions still open (`input-decisions.md`): D5 (the input channel's
  overflow policy), D6 (repeat and keymap placement) and D10 (touch).

## C2: the cursor under QEMU

- Check the cursor sprite against injected pointer motion under QEMU raspi3b.
- Exit: a `just` recipe moves the pointer by injected motion and checks the
  cursor's position on the framebuffer.

## C5: real input on the board

- Hardware check: a USB mouse moves the cursor and clicks focus a window, and a
  keyboard reaches the focused client, on the board's display.
- Exit: moving a mouse moves the cursor on the board's screen.

## C6: later features

- Resize negotiation (`ResizeOffer`, new surface, swap), an overlay layer for a
  panel, and page flipping if the framebuffer offers a virtual height. Alpha
  blending is in version 1 (Q6), so it is not a C6 item.
- A separate `inputd`-style router is future work, not a stage (see the Future
  work section of `ui-compositor-design.md`).
- Each is its own slice with its own decision if the answer is still open.

## Verification across stages

- Host tests for `userspace/libs/compositor-core` at every stage
  (`just _test-compositor-host`).
- `just clippy` and `just fmt-check` for every Rust change, as the project's
  `just` workflow requires.
- Interface checks with `meddler check` for each `.if` change.
- QEMU checks through `just` recipes for C2 and C5. A missing recipe is a
  blocker for that stage, not a pass.
