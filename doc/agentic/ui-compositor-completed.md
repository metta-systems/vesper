# UI compositor completed

Short markers of what is done. Each line says what exists and where it is exercised.
Open items stay in [ui-compositor-implementation-plan.md](ui-compositor-implementation-plan.md);
the design is in [ui-compositor-design.md](ui-compositor-design.md). No compositor
decision is open.

## Completed decisions

Each is recorded in [ui-compositor-design.md](ui-compositor-design.md).

- [x] Q1 (2026-10-19): the compositor carves the surface Frames and installs them in
  the client's lent table; the client just draws.
- [x] Q2 (2026-10-18, rule 2026-10-19): granularity by size; a surface is its bytes
  rounded up to 2 MiB, made of 2 MiB Frames.
- [x] Q3 (2026-10-18): one event io-channel per client, the window id in each record.
- [x] Q4 (2026-10-18): the compositor routes input itself; an `inputd`-style router is
  future work.
- [x] Q5 (2026-10-18): client-side decorations; the compositor runs the Alt-drag grab.
- [x] Q6 (2026-10-18): ARGB with premultiplied blending from version 1; hit-testing by
  window rectangles.
- [x] Q7 (2026-10-18, rule 2026-10-19): a fixed 33 ms timer that runs only while damage
  exists; the adaptive rate is deferred.
- [x] Q8 (2026-10-18): `uses ... via device ...` and `uses ... via component ...`.
- [x] Q9 (2026-10-18): the component lives in `userspace/gui/compositor`.
- [x] Q10 (2026-10-18): the input decisions the routing needed (input D1 to D4, D13)
  were answered first.
- [x] Q11 (2026-10-18): a client channel's overflow drops the newest delivery, emits
  `Dropped` and resyncs from state.
- [x] Q12 (2026-10-27): a new window gets the focus; a click still moves it.

## C4: per-client event channel and routing

- [x] One event io-channel per client (Q3): `compositor_v1` declares `channel 1;`, so Kickstart gives every importer a control page, a data page and an EventCount; the compositor delivers each routed frame's records for the client's windows (window id in `subject`, each window's records closed by a `Sync`) under its turn, drops a delivery whole when no buffer fits and starts the next with `Dropped` (Q11), and `compositor_v1.snapshot` rebuilds the client's focus, held keys and pointer; a new window gets the focus (Q12). `userspace/libs/compositor-core` (`events`, `Router::focus_window`, `Router::snapshot`) is host-tested by `just _test-compositor-host`; `just _test-window-events` checks focus on creation, click to focus, a drag across windows and a key at the client `window-events` (QEMU monitor input on the USB mouse and keyboard, not an injected loopback producer), which exits with the result; `just _test-wood` checks that Wood receives keys.
