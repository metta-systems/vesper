# Logging implementation plan

Design: [logging-design.md](logging-design.md) (with the answered decisions L1 to L10).
Open decisions: [logging-decisions.md](logging-decisions.md). This file holds only open
work and the checks of what was built; mark an item `- [x]` only after its exit check
has passed.

## Dependencies

- L1 to L10 are answered (2026-10-28); the maintainer reviews them afterwards, so a
  change there may reopen built items. "Needs review" items: the `(N dropped)` prefix
  (L3) and the image-build `interfaces` key (L4).
- L11 (console input) is answered (2026-10-29): received bytes go into the input
  events, and input decision D8 (2026-10-29, a) turns them back into the shell's
  bytes through the console adapter (input stage INPUT3).
- L-D (panics) is set aside by the maintainer for now.
- Board: L-A's hardware exit is pending.

## L-A: the log server in wood-test, first clients, music diagnostics

Built (2026-10-28):

- `userspace/log-server/interfaces/log_v1.if` (`channel 2;`, `flush()`); generated
  crates `userspace/log-server/server` (`log-interface`) and `client`
  (`log-interface-client`).
- `userspace/log-server`: library (channel layout, `Header`, `FreeBuffers`,
  `LineWriter`, `format_line`) and the component (one Thread, round robin, serial
  buffers of 1 KiB, `flush` advancing its `EventCount`).
- `libs/log-client` (`liblog_client`): `send`, `Logger` (`log::Log`), `log_println!`.
- `libs/serial-trace`: `Trace::line` and `note` send on the log channel first.
- Clients: compositor, Wood (`logger.rs`), `midi_synth` and the audio driver import
  `log_v1`; Wood also imports `mailbox_v1.get_clock_rate`.
- `kernel/tests/wood-test`: `log_server` with `serial = true` (moved from the
  compositor); the test kernel loads it and checks its 3 + PWM-node clients, the
  compositor without a serial channel, and the new imports.
- `libs/image-build`: top-level `interfaces` key; set in kernel/kickstart,
  compositor-test, window-events-test and audio-test, whose clients' imports stay
  unlinked.
- Wood's music summary every 256 periods and the clocks at music start (design doc,
  section Music diagnostics).
- Justfile: `build-wood` builds `log-server`; `test-wood` writes the UART to
  `target/wood-test-serial.log` and checks it; `test-log-host`, `clippy-log`.

Exit checks:

- [x] `just test-log-host`: 12 host tests of the header, buffer set, line cutting and
  output line pass.
- [x] `just test-wood`: passes as before, and the UART file holds the server's start
  line, the compositor's display and device lines, `wood: the engine started`, Wood's
  music summary, `midi-synth: ready at 44091710 mHz` and the audio driver's
  `open for client 0`, each with the `[seconds] I ` prefix (0 lines dropped in the
  runs so far).
- [x] Unchanged behaviour where the import stays unlinked: `just test-compositor`,
  `just test-window-events`, `just test-audio`, `just test-shell`,
  `just test-usb-core`, `just test-usb-hid` pass.
- [x] `just build-wood-test-hw` builds.
- [x] Lint: `just clippy-log` (the log crates and `libserial-trace`, with and without
  `qemu`, and the host tests); the target build of Wood, `midi-synth`, the compositor,
  the audio driver and the USB components with and without `qemu`, and
  `vesper-image-build`, all with `--allow clippy::map_unwrap_or --allow
  clippy::doc_markdown` for the known `libmeddle` breakage (`clippy-log` passes them
  too); `just fmt-check`.
- [ ] Hardware exit: `just chainboot_serial=/dev/ttyUSB0 chainboot_load_baud=1500000
  boot-wood-test` on the Pi 3 B (the overlay line in `config.txt`, headphones in):
  the terminal shows `log-server: 4 clients, serial output attached`, then the
  compositor's, Wood's, the synthesiser's and the audio driver's lines with the
  timestamp prefix. What to read for the music:
  - `wood: music: ARM clock N MHz, core clock N MHz`: the clocks the render runs at
    (the A5a measurement this gives directly);
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

## L-B: every component a client, relays removed

Depends on L-A's hardware exit only for confidence; no open decision.

- [ ] Raise `MAX_AREAS` in `libs/device-driver` (and the init page layout checks) so the
  log server can serve every component of wood-test (13 or more). With the USB
  components wood-test's log server has 7 clients of the 8 (the wood-test kernel
  checks the count against `MAX_AREAS`); the next client needs the raise.
- [x] The USB components are clients (2026-10-29): `usb-hcd-dwc2`, `usb_core` and
  `usb_hid` import `log_v1` (`interfaces/imports.if`); usb-core-test, usb-hid-test and
  usb-test list the log server's interface directory (top-level `interfaces`) so the
  imports stay unlinked there, as in kernel/kickstart, compositor-test and
  window-events-test. The volume policy is in the design doc (section USB log
  volume). Kickstart's driver Thread stacks went from 16 to 32 KiB (`STACK_PAGES`):
  `usb-core` overflowed 16 KiB under QEMU once its trace went to the log channel.
  Checked: `just test-wood` (the UART file holds the controller's bring-up and port
  reset, a device the core enumerated and `usb-hid: serving a Keyboard`; 38 USB
  lines in a 12.5 s run, 33 of them in the first 0.76 s, 0 dropped); test-usb, test-usb-core, test-usb-hid,
  test-compositor, test-shell (all run by hand, logs in `target/usbclient-*.log`),
  test-window-events, test-audio, test-midi, test-dispatch, test-driver, test-mailbox,
  test-uart; `just build-wood-test-hw`; clippy of the USB crates, `libserial-trace`,
  Kickstart and the wood-test kernel with and without `qemu`; `just clippy-log`,
  `just clippy-usb-host`. Board: pending (L-C).
- [ ] Add the `log_v1` import to mailbox, fb, gpio, cprman, dma and `draw_demo`; add
  the log server to compositor-test, window-events-test, audio-test, usb-core-test and
  usb-hid-test (each recipe's serial-file check as in `test-wood`).
- [ ] Remove `libserial-trace`'s backlog, `drain` and the `trace(octet&)` exports of
  `input_v1`, `usb_v1` and `usb_hcd_v1`, and the relays in `input_trace` and
  `usb_core` (no backwards compatibility; the interfaces are prototypes and are edited
  in place).
- [ ] Check at image build time that exactly one component has `serial = true`.
- Exit: each changed image's recipe passes with its UART file holding every client's
  lines; `just test-log-host`.

## L-C: production image and hardware

- [ ] Serial RX into the input events (L11): the log server attaches the receive side
  and turns received bytes into input records the compositor routes; the shell reads
  them as bytes through the console adapter (input D8 a, INPUT3).
- [ ] Production image (`kernel/kickstart/image.toml`) with a log server: after the
  RX item above.
- [ ] Hardware exits of the L-B images on the Pi 3 B.

## L-D: panics and early output

Set aside by the maintainer for now.

- [ ] The EL0 panic handler (`userspace/runtime`) puts `PANICKED: …` on the
  component's log channel (best effort: `liblog_client::send`, which never waits)
  before halting; check with a component made to panic in a test image.
- [ ] A line from the log server when a client's channel shows a malformed packet or
  a large drop count, so a misbehaving client is named.

## Verification commands

| Command | What it checks |
|---|---|
| `just test-log-host` | the log protocol library |
| `just test-wood` | the log server in QEMU, its clients' lines on the UART file, Wood's music |
| `just build-wood-test-hw` | the board image builds |
| `just clippy-log` | the log crates' lint, allowing the known `libmeddle` breakage |
