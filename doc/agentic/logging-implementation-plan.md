# Logging implementation plan

Design: [logging-design.md](logging-design.md), with the decisions LOGGING-1 to
LOGGING-14; open questions are in [logging-decisions.md](logging-decisions.md)
(Q-LOGGING-1, the DebugConsole byte transport). This file holds only open work;
mark an item `- [x]` only after its exit check has passed, and move it to
[logging-completed.md](logging-completed.md). Done: P-LOGGING-1, P-LOGGING-2,
P-LOGGING-4 and P-LOGGING-5, each with an open rest below.

## Dependencies

- LOGGING-1 to LOGGING-10 are answered (2026-10-28); the maintainer reviews them
  afterwards, so a change there may reopen built items. "Needs review" items: the
  `(N dropped)` prefix (LOGGING-3) and the image-build `interfaces` key (LOGGING-4).
- LOGGING-11 (console input): received bytes go into the input events, and
  INPUT-8 turns them back into the shell's bytes through the console adapter
  (P-INPUT-3).
- Board: P-LOGGING-6's hardware exit is pending.

## P-LOGGING-6: the log server in wood-test, on the board

The rest of P-LOGGING-1.

- [ ] Hardware exit: `just chainboot_serial=/dev/ttyUSB0 chainboot_load_baud=1500000
  boot-wood-test` on the Pi 3 B (the overlay line in `config.txt`, headphones in):
  the terminal shows `log-server: 4 clients, serial output attached`, then the
  compositor's, Wood's, the synthesiser's and the audio driver's lines with the
  timestamp prefix. What to read for the music:
  - `wood: music: ARM clock N MHz, core clock N MHz`: the clocks the render runs at
    (the P-AUDIO-9 measurement this gives directly);
  - `wood: music: summary of 256 periods: service min/avg/max …`: the synthesiser's
    render time per period against the 11 612 µs period; an average above a few
    thousand µs, or maxima near the period, explain bad music;
  - `refill max R us of a 11612 us period` and `underruns U (+D)`: a growing `+D`
    means periods were filled with silence;
  - `packets handled`, `F failed`, `playing`: whether the synthesiser takes the
    packets and thinks a song plays;
  - `midi-synth: a song of B bytes plays` / `does not parse` / `packet … refused` and
    `wood: music: song N of B bytes sent`: whether the songs load;
  - `(N dropped)` before a line: the UART did not keep up (try a lower log volume or
    a higher rate).
- [ ] Measure the PL011's transmit interrupt rate at 1.5 Mbaud while the log is busy
  (design doc, Interactions and limits); raise the FIFO interrupt level if it costs the
  playback Thread time.

## P-LOGGING-7: every component a client, relays removed

The rest of P-LOGGING-2. Depends on P-LOGGING-6's hardware exit only for
confidence; no open decision.

- [ ] Raise `MAX_AREAS` in `libs/device-driver` (and the init page layout checks) so the
  log server can serve every component of wood-test (13 or more). With the USB
  components wood-test's log server has 7 clients of the 8 (the wood-test kernel
  checks the count against `MAX_AREAS`); the next client needs the raise.
- [ ] Add the `log_v1` import to mailbox, fb, gpio, cprman, dma and `draw_demo`; add
  the log server to compositor-test, window-events-test, audio-test, usb-core-test and
  usb-hid-test (each recipe's serial-file check as in `test-wood`).
- [ ] Remove `libserial-trace`'s backlog, `drain` and the `trace(octet&)` exports of
  `input_v1`, `usb_v1` and `usb_hcd_v1`, and the relays in `input_trace` and
  `usb_core` (no backwards compatibility; the interfaces are prototypes and are edited
  in place).
- [ ] Check at image build time that exactly one component has `serial = true`.
- Exit: each changed image's recipe passes with its UART file holding every client's
  lines; `just _test-log-host`.

## P-LOGGING-3: production image and hardware

- [ ] Serial RX into the input events (LOGGING-11): the log server attaches the receive side
  and turns received bytes into input records the compositor routes; the shell reads
  them as bytes through the console adapter (INPUT-8, P-INPUT-3).
- [ ] Production image (`kernel/kickstart/image.toml`) with a log server: after the
  RX item above.
- [ ] Hardware exits of the P-LOGGING-2 and P-LOGGING-7 images on the Pi 3 B.

## P-LOGGING-8: panics and faults, the rest

The rest of P-LOGGING-4. Design: section Panics and faults.

- [ ] Board: a `FAULT:` or `PANICKED:` line on the Pi 3 B's UART (the next boot of
  wood-test that hits one; nothing forces one there).
- [ ] A line from the log server when a client's channel shows a malformed packet or
  a large drop count, so a misbehaving client is named.

## P-LOGGING-9: serial console at boot, on the board

The rest of P-LOGGING-5 (LOGGING-12).

- [ ] Board: wood-test from the SD card with `just console`, and through
  `just boot-wood-test` (handoff), on the Pi 3 B.

## P-LOGGING-10: DebugConsole general availability

**Parked** (maintainer decision): DebugConsole stays a debug-only prototype ([LOGGING-13](logging-design.md#logging-13-debugconsole-is-a-debug-only-exception)). Every item below is console general-availability work. It waits on the byte-transport question [Q-LOGGING-1](logging-decisions.md#q-logging-1-debugconsole-byte-transport) and the console authority decision under capability D4, and does not block other work.

- [ ] Route user-copy fault recovery through the correct exception path, without recursive capability dispatch. Lands with the console's checked user-memory access below.
- [ ] Define the console operation's authority, byte/string/NUL behavior, maximum length or chunking policy, and pointer semantics.
- [ ] Introduce checked user-memory access for the console path, including caller-context authorization, range/length/overflow validation, input stability, and fault behavior. Do not relabel caller virtual addresses as trusted physical addresses.
- [ ] Bound console copying and terminator handling; return specified errors for malformed inputs rather than panicking.
- [ ] Test console success, kernel error propagation, invalid/empty slots, excessive raw slot/op values, boundary lengths, invalid/unauthorized pointers, non-SVC faults, and fault recovery without recursive capability dispatch.

## Verification commands

| Command | What it checks |
|---|---|
| `just _test-log-host` | the log protocol library |
| `just _test-wood` | the log server in QEMU, its clients' lines on the UART file, Wood's music |
| `just _test-log` | a probe's stack overflow and panic reach the UART as `FAULT:` and `PANICKED:` lines |
| `just _build-wood-test-hw` | the board image builds |
| `just _clippy-log` | the log crates' lint, allowing the known `libmeddle` breakage |
