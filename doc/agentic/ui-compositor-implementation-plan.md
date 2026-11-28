# UI compositor implementation plan

Design and its decisions: `ui-compositor-design.md`. Completed items:
`ui-compositor-completed.md`. Nothing here starts until the decisions it depends on
are recorded. Input stages are in `input-implementation-plan.md`; multiple outputs
are in `multi-display-implementation-plan.md`.

## Dependencies

- No compositor decision is open (`ui-compositor-design.md`, UI-COMPOSITOR-1 to
  UI-COMPOSITOR-12).
- Input decisions still open (`input-decisions.md`): Q-INPUT-1 (the input
  channel's overflow policy), Q-INPUT-2 (repeat and keymap placement) and
  Q-INPUT-3 (touch).

## P-UI-COMPOSITOR-7: the cursor under QEMU

- Check the cursor sprite against injected pointer motion under QEMU raspi3b.
- Exit: a `just` recipe moves the pointer by injected motion and checks the
  cursor's position on the framebuffer.

## P-UI-COMPOSITOR-8: real input on the board

- Hardware check: a USB mouse moves the cursor and clicks focus a window, and a
  keyboard reaches the focused client, on the board's display.
- Exit: moving a mouse moves the cursor on the board's screen.

## P-UI-COMPOSITOR-6: later features

- Resize negotiation (`ResizeOffer`, new surface, swap), an overlay layer for a
  panel, and page flipping if the framebuffer offers a virtual height. Alpha
  blending is in version 1 (UI-COMPOSITOR-6), so it is not part of this.
- A separate `inputd`-style router is future work, not a stage
  (N-UI-COMPOSITOR-1 in `ui-compositor-design.md`).
- Each is its own slice with its own decision if the answer is still open.

## Verification across stages

- Host tests for `userspace/libs/compositor-core` at every stage
  (`just _test-compositor-host`).
- `just clippy` and `just fmt-check` for every Rust change, as the project's
  `just` workflow requires.
- Interface checks with `meddler check` for each `.if` change.
- QEMU checks through `just` recipes for P-UI-COMPOSITOR-7 and
  P-UI-COMPOSITOR-8. A missing recipe is a blocker for that stage, not a pass.
