# Logging completed

Short markers of what is done. Open work stays in
[logging-implementation-plan.md](logging-implementation-plan.md); the design, with each
decision and the alternatives weighed, is in [logging-design.md](logging-design.md).

## Completed decisions

- [x] L1 to L10 (2026-10-28): `logging-design.md`, section Decisions.
- [x] L11 (2026-10-29): the log server forwards console input into the input events;
  `logging-design.md`, section Decisions.
- [x] L12 (2026-10-29): the serial console negotiates its rate at boot, as chainboot
  does; `logging-design.md`, section Decisions.

## DebugConsole

- [x] DebugConsole gated behind `debug_kernel` (handler, grant, boot use); its results are propagated and its wrapper no longer prints.
- [x] Hardware debugging aid: in a `debug_kernel` kernel every component Kickstart loads gets a `WRITE` console key (`DriverInit::debug_console`, slot 15) its panic handler prints on, and Write reads caller-virtual memory checked by `AT` at the caller's level and prints on the board UART outside QEMU (`kernel/nucleus/src/objects/debug_console.rs`, `kernel/nucleus/src/board_console.rs`, `userspace/runtime`); exercised by the nucleus `debug_console` suite, `capability-test` and `ppc-test`, and on the Raspberry Pi 5 by board-test's log probe (`just boot-board-test debug_kernel`); EL0 bytes are read with `LDTRB` because the Pi 5's Cortex-A76 sets PAN on exception entry.
