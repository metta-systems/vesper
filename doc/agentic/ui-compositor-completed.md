# UI compositor completed

Short markers of what is done. Each line says what exists and where it is exercised.
Open items stay in [ui-compositor-implementation-plan.md](ui-compositor-implementation-plan.md)
and [ui-compositor-decisions.md](ui-compositor-decisions.md); the design is in
[ui-compositor-design.md](ui-compositor-design.md).

## C4: per-client event channel and routing

- [x] One event io-channel per client (Q3): `compositor_v1` declares `channel 1;`, so Kickstart gives every importer a control page, a data page and an EventCount; the compositor delivers each routed frame's records for the client's windows (window id in `subject`, each window's records closed by a `Sync`) under its turn, drops a delivery whole when no buffer fits and starts the next with `Dropped` (Q11), and `compositor_v1.snapshot` rebuilds the client's focus, held keys and pointer; a new window gets the focus (Q12). `userspace/libs/compositor-core` (`events`, `Router::focus_window`, `Router::snapshot`) is host-tested by `just test-compositor-host`; `just test-window-events` checks focus on creation, click to focus, a drag across windows and a key at the client `window-events` (QEMU monitor input on the USB mouse and keyboard, not an injected loopback producer), which exits with the result; `just test-wood` checks that Wood receives keys.
