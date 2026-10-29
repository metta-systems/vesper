# Logging design (several components, one PL011)

Status: decisions L1 to L10 answered on 2026-10-28 and L11 on 2026-10-29 (section
Decisions). Stage L-A (the
log server in wood-test with the compositor, Wood, the MIDI synthesiser and the audio
driver as clients, and Wood's music summary) is built and checked in QEMU; the board
check is pending. No logging question is open
([logging-decisions.md](logging-decisions.md)); stages are in [logging-implementation-plan.md](logging-implementation-plan.md).
`log_v1` is a prototype interface and is edited in place.

The first use is diagnosing Wood's music on the board: the sound effects play, the
music (the `midi_synth` component, rendering inside Wood's playback Thread's `service`
call) seems not to work well, and nobody could see why, because Wood's and
`midi_synth`'s lines were dropped on hardware.

## Decisions

The maintainer answered on 2026-10-28: "go with all recommended, my guidance -
log_server component, connected to serial output; compositor, wood, synth all clients of
logserver, non-blocking operation (client notifies logserver of new entries through
eventcount perhaps, but if there's no free record to send the log line it is silently
dropped). write down decisions made and possible alternatives, i will check those
afterwards." Each decision below is the option the decisions file recommended unless
the guidance said otherwise; the alternatives that were weighed follow each one, for
review. Items marked **needs review** were settled during implementation without a
question to the maintainer.

- **L1. Transport: one `log_v1` io-channel per client.** Each client owns its
  channel's data pages as 32 buffers of 256 bytes; a line is one packet of one record.
  Not chosen: a PPC `write(octet& text)` per line, copied by the export into a ring of
  the server's (a system call per line, and the copy runs on the client's Thread
  inside the server); one multi-producer ring all clients map (cheapest, but a client
  can overwrite another's lines, against the confinement the capability design keeps).
- **L2. Wake-up: the client calls `flush()` after putting a line; `flush` advances the
  server's own `EventCount` (`DriverInit::event_count`) on the client's Thread and
  returns.** The server's Thread sleeps on that count with a 20 ms timeout, so a line
  is written even when `flush` fails (for example `NoFreeStack`). This is the
  maintainer's "client notifies logserver of new entries through eventcount" with no
  Kickstart change: the `channel N;` wiring only gives the exporter a `SEND` key and the
  client a `RECV` key of the pair's `EventCount`, so the client cannot advance that one.
  Not chosen: Kickstart giving each client a badged `SEND` key of one Notification and
  the server a WaitSet over it and the serial completion (immediate, no PPC, up to 64
  clients, but a Kickstart and Meddle extension); a client-advanced `EventCount` per
  channel with a WaitSet (8 members per set); polling only.
- **L3. Overflow: the client drops, never waits.** A line that finds no free buffer, or
  another Thread of the same component in the middle of sending (a try-lock), is
  dropped. Per the guidance the drop is silent for the client. The client counts the
  drops and the count rides in the next line's header; the server then prefixes that
  line with `(N dropped)`. **Needs review:** the count and the prefix are an addition
  the maintainer can veto (the alternative is a truly silent drop). The server never
  waits on a client either: when every serial buffer is in flight it stops taking
  lines, so back-pressure lands in the clients' channels. Real-time Threads (Wood's
  playback Thread, and `midi_synth` running on it during `service`) log through the same
  path. Not chosen: blocking for a buffer as an opt-in for non-real-time Threads;
  always blocking; dropping the oldest line.
- **L4. How clients get the channel: an explicit import** in the component's
  `imports.if`: `uses log_v1 via component log_server { flush }`. An image without the
  log server leaves the import unlinked and the component falls back as before
  (semihosting, the serial channel or the backlog). **Needs review (image-build
  extension):** an image must find the interface file of every import, so images that
  bundle a client but not the log server list the log server's interface directory in
  a new top-level manifest key, `interfaces = ["…/userspace/log-server/interfaces"]`
  (`libs/image-build`, `searched_interface_directories`): kernel/kickstart,
  compositor-test, window-events-test and audio-test. Not chosen: a Kickstart default
  log channel for every component; a manifest flag (`log = true`) replacing
  `serial = true`.
- **L5. Client identification: the client's own prefix in the text,** as every line
  already had (`wood: …`, `midi-synth: …`, `compositor: …`, `audio: …`); the server adds
  no name. `liblog-client`'s `Logger` adds the component name it is built with. Not
  chosen: a trusted label Kickstart records per pair, as Genode's parent-set session
  label (needs the exporter to learn its clients' names, a Kickstart change); a name
  the client declares once in an `open(name)` call (spoofable, no gain over the text).
- **L6. Record format: a header in the packet's value word, formatted by the server.**
  Level (the `log` crate's numbering, 0 for none), a cut flag, the client's drop count
  (16 bits, saturating) and the client's clock in microseconds (40 bits). The server
  writes `[     12.345678] I wood: text\r\n` (`-` for no level, `…` after a cut line)
  and replaces control bytes with `?`, so a client cannot move the terminal's cursor or
  fake a line start. Not chosen: the client formats the whole line (no common
  timestamp or level format); no header (no drop count, no level).
- **L7. Serial input: the log server attaches only the transmit side;** received bytes
  are dropped and counted by the PL011 driver, as in every image where nobody reads
  RX. Images whose shell reads keys (the production image) keep the shell as the
  serial holder and have no log server yet; once the production image gets one,
  received bytes go into the input events (L11). Not chosen for now: the log
  server forwarding RX to one client named in its manifest; the shell as a raw,
  unprefixed passthrough client.
- **L8. Early output and panics:** Kickstart wires every channel before any Thread
  starts, so a client's first lines wait in its channel until the server runs (up to
  32 lines). Panics stay on the semihosting console (`userspace/runtime`) for now; a
  best-effort panic line through the log channel is a later stage (L-D). The nucleus
  and Kickstart stay semihosting-only. Not chosen: a polled direct-UART fallback in
  components (breaks the one-owner rule); a kernel log object such as Fuchsia's
  debuglog (minimal kernel API).
- **L9. QEMU: both semihosting and serial.** `liblog-client`'s `Logger` and
  `libserial-trace` keep printing on the semihosting console under `qemu`, so every
  recipe's checks hold; `just test-wood` sends the UART to its own file
  (`target/wood-test-serial.log`, `-serial file:`) so serial lines are never split by
  semihosting output, and checks the server's lines there. Not chosen: serial only
  under QEMU (rewrites every recipe's checks); semihosting only (the log server is not
  exercised).
- **L10. Naming:** component `log_server` (crate and binary `log-server`), interface
  `log_v1`, generated crates `log-interface` and `log-interface-client`, client library
  `vesper-log-client` (`liblog_client`; `liblog` is taken by `vesper-log`). Not chosen:
  `console`/`console_v1` (the input side is not part of it yet), `syslog`.
- **L11. Console input: a, the log server forwards RX, into the input events.**
  Answered 2026-10-29: "L11 - a, it should go to the input-event". The log server
  owns both directions of the PL011; received bytes are not dropped but enter the
  input event path ([input-design.md](input-design.md)), so serial typing reaches the
  focused consumer the way USB keyboard input does. How the bytes become events (the
  log server as an `input_v1` producer the compositor imports next to `usb_hid`, or a
  byte channel to an input adapter) is implementation work; input decision D8
  (answered 2026-10-29: the console adapter presenting a serial-compatible byte
  channel, [input-design.md](input-design.md)) turns the events back into the shell's
  bytes. Not chosen: two UARTs
  (b); an output-only serial port (c). Panic output (L-D) is set aside for now by the
  maintainer ("ignore panic for now").

## Current state before the log server

### Who can write to the UART

- **One serial channel per image.** A component with `serial = true` in `image.toml`
  (`libs/image-build/src/lib.rs`, field `serial`) is attached to the first PL011
  instance's channel by Kickstart (`kernel/kickstart/src/drivers.rs`, section
  "Serial channel"; `kernel/kickstart/src/serial.rs`, `SerialChannelParts`): "the
  first such program gets the first PL011 instance". A second `serial = true` component
  is ignored.
- **The channel** (`libs/serial-channel`) is a control page with two `libio-channel`
  channels (`transmit`, `receive`, 32 slots each), a 4-page data area at fixed
  addresses (`CONTROL_VA` 0x3802_0000, `DATA_AREA_VA` after it), and three
  `EventCount`s: the client advances `transmit_submitted`, the driver advances
  `transmit_completed` and `receive_completed` (keys in `DriverInit::serial`).
- **The PL011 driver** (`drivers/uart-pl011`) has the two Threads of the IO channel
  design (interrupt and transmit), writing `TRANSMIT_BURST` (8) bytes per transmit
  interrupt, at the baud rate chainboot left. Without a client the UART stays quiet.
- **Holders:** the shell in the production image (the only component reading serial
  input), the compositor in compositor-test and window-events-test (and wood-test
  before L-A), the audio driver in audio-test, `usb_core` in usb-core-test,
  `input_trace` in usb-hid-test.
- **Baud rate:** chainboot starts at 115 200 (`libs/chainboot-protocol`,
  `BOOT_BAUD_RATE`); chainofcommand can switch it for the load and the console stays at
  that rate (`chainboot_load_baud` in the `Justfile`).

### How components print

| Mechanism | Where | On QEMU | On the board |
|---|---|---|---|
| `semi::println!` (`libuser::semihosting`) | everywhere, incl. Kickstart's `🥾` lines | semihosting console | nothing |
| `libserial_trace::Trace` + `trace!` | compositor, audio driver, `usb_core`, `input_trace` | semihosting and the serial channel | the serial channel if held, else the backlog |
| `libserial_trace::note` + backlog | `midi_synth`, `usb_hid`, `usb-hcd-dwc2` | semihosting, plus a 4 KiB backlog | the backlog only, unless a holder pulls it |
| backlog pull exports `trace(octet&)` | `input_v1`, `usb_v1`, `usb_hcd_v1` | pulled by `input_trace` / `usb_core` | an ad hoc relay, one pair at a time |
| `log` crate, `userspace/wood/vesper/src/logger.rs` | Wood | semihosting, `wood: LEVEL text` | nothing (`enabled()` was false without `qemu`) |
| `libconsole` / `vesper-log` | nucleus, chainboot | semihosting | chainboot only; nucleus output and panics are silent |
| `DebugConsole` kind | debug kernels | semihosting | nothing |
| EL0 panic handler (`userspace/runtime`) | every component | `EL0 PANICKED: …`, exit | silent `halt()` |

`Trace::line` waits for each line to be sent (`SerialLog::send`); the backlog never
waits and drops silently when full or contended.

### What io-channels and the kernel offer

`channel N;` in an interface makes Kickstart carve, per (client, exporter) pair, a
control page and N data pages at one address in both, and one `EventCount` (exporter
`SEND`, client `RECV`); the exporter finds the pair in `DriverInit::areas` (at most
`MAX_AREAS` = 8), the client in its `ImportRecord`s. Nothing lets the client wake the
exporter. The kernel offers `EventCount.Await` with a timeout, badged `Notification`
signals, `WaitSet` (8 members) and PPC `Invocation`s that run on the caller's Thread
(16 export stacks per exporter in Kickstart; a full pool fails with `NoFreeStack`).
No new kernel kind or operation is needed for any option here.

## Requirements

1. Several clients, one serial output, one owner of the PL011 channel.
2. Never block a real-time Thread: a log call costs a bounded copy and one
   non-blocking call at most.
3. Line atomicity: clients interleave only at line boundaries.
4. Client identification, level and time on each line.
5. Visible, fair back-pressure: a flooding client cannot starve the others.
6. Throughput: 8N1 at 115 200 baud is 11 520 bytes/s (about 140 80-byte lines/s);
   1.5 Mbaud is 150 000 bytes/s. Periodic diagnostics must be summaries, not a line
   per 11.6 ms period.
7. Early lines are kept until the server runs.
8. Panics reach the UART when the log path works (later stage).
9. Lines of one client stay in order.
10. QEMU recipes keep working.
11. An image whose shell reads the serial port keeps working.
12. No new kernel kinds or operations.

## Architecture (as built in L-A)

```
 compositor   wood (game, playback)   midi_synth   audio driver     (clients)
     |  log_v1 io-channel: one line per packet, header in the value word
     v           v                       v             v
 +--------------------------------------------------------------+
 | log_server: one Thread, round robin, one line per client     |
 | per pass, formats into 1 KiB serial buffers                  |
 +--------------------------------------------------------------+
                 | libserial-channel transmit (serial = true)
                 v
             uart-pl011 -> PL011
```

### `log_v1` (`userspace/log-server/interfaces/log_v1.if`)

`local interface log_v1` with `channel 2;`, `badged;` and one function, `flush()`. The
protocol constants and pure logic live in the `log-server` library
(`userspace/log-server/src/lib.rs`, host-tested in `tests/protocol.rs`): the channel
type (`LogChannel`, 64 slots per FIFO so every buffer can be in flight), `LINE_BYTES`
256, `BUFFERS` 32, `Header` encode/decode, `FreeBuffers`, `LineWriter` (cuts at a
character boundary, drops trailing line ends) and `format_line`.

### The server (`userspace/log-server`)

One active Thread (`src/component.rs`):

1. Finds its clients in `DriverInit::areas` and the serial channel in
   `DriverInit::serial`; writes `log-server: N clients, serial output attached`.
2. Each pass takes back finished serial buffers, then takes at most one line from each
   client in turn, checks the packet (one record, at most 256 bytes, inside that
   client's data pages), formats it into the serial buffer being filled (1 KiB each,
   16 buffers, so the 32-slot transmit FIFO holds all of them) and hands the client's
   packet back. A malformed packet comes back with its length zeroed.
3. Puts the filled buffer and advances `transmit_submitted` once per pass.
4. With every serial buffer in flight it waits on `transmit_completed` (20 ms timeout);
   with nothing to do it waits on its own `EventCount` (advanced by `flush`) with the
   same timeout.

The server's export `flush` only advances its `EventCount`.

### The client library (`libs/log-client`, `liblog_client`)

- `send(level, arguments) -> Sent` (`Put`, `Dropped`, `NoChannel`): attaches lazily from
  the init record at `INIT_VA` (the `log_v1` import's channel), takes the try-lock,
  takes back returned buffers, formats into a free buffer, puts the packet, releases
  the lock and calls `flush`. Never waits. Marked `#[inline(never)]`: inlined into
  `libserial-trace`'s callers it overflowed `usb-core`'s 16 KiB stack in
  window-events-test.
- `Logger::new("wood")`: a `log::Log` that sends `wood: text` at the record's level and,
  under `qemu`, prints `wood: INFO text` on semihosting as before.
- `log_println!` (Info) and `line(level, arguments)`.
- `libserial-trace` sends through it first: `Trace::line` and `note` use the log
  channel when the component has one, then the serial channel, then the backlog. So
  every `Trace`/`note` user becomes a client by adding the import.

### Clients in wood-test

- **Compositor:** `uses log_v1` added to `userspace/gui/compositor/interfaces/imports.if`;
  `serial = true` moved from it to `log_server` in `kernel/tests/wood-test/image.toml`.
- **Wood:** `userspace/wood/vesper/src/logger.rs` is a `liblog_client::Logger`; both
  Threads log through it (`enabled()` is now true on the board).
- **MIDI synthesiser:** `userspace/wood/midi-synth/interfaces/imports.if` imports
  `log_v1`; its `note` lines (including those written during `service` on Wood's
  playback Thread) go to its own channel.
- **Audio driver:** imports `log_v1` too; its `Trace` lines reach the UART.

### Music diagnostics (Wood)

- At music start: `wood: music: ARM clock N MHz, core clock N MHz (0: unknown)` through
  `mailbox_v1.get_clock_rate` (Wood imports it; one call each, before streaming).
- Every 256 periods (about 3 s) from the playback Thread:
  `wood: music: summary of 256 periods: service min/avg/max A/B/C us over N calls (F failed), P packets handled, S periods with sound, playing true; refill max R us of a 11612 us period; underruns U (+D)`.
  `service` is the synthesiser's render call; "refill" is the whole period's work
  (music pull, mixing, packet put); the underrun count is the driver's, with the
  increase since the last summary. Without music the line starts with `audio:
  summary`.
- The existing lines stay: `music: song N of B bytes sent`, `midi-synth: a song of B
  bytes plays` / `does not parse` / `packet … refused`, the start hash, and the
  power-of-two `music: … periods pulled` and `audio: … buffers mixed` reports.

Under QEMU the summary reads, for example, service min/avg/max 124/274/8856 µs and
refill max 8881 µs of 11 612 µs: the averages fit, the maxima are QEMU's scheduling
jitter. On the board the same line shows whether the synthesiser's render fits the
period at the ARM clock it runs at.

### Interactions and limits

- **Kickstart limits.** `MAX_AREAS` (8 data areas per exporter) caps the log server at 8
  clients; wood-test uses 4. Migrating the USB components and the rest needs it raised
  (an `ExportedArea` is 32 bytes of the init page). Each channel takes 3 pages of the
  2 MiB data window shared by all pairs.
- **PL011 interrupt load at high baud.** At 1.5 Mbaud an 8-byte transmit burst drains
  in 53 µs: up to about 19 000 transmit interrupts per second while the log is busy;
  worth measuring before trusting timing measured at that rate. The FIFO level
  (`IFLS`) is the knob.
- **Stack depth.** `usb-core`'s Thread runs close to its 16 KiB stack; tracing from deep
  call chains must keep frames small.
- **libserial-trace relays.** The backlog and the `trace(octet&)` exports of `input_v1`,
  `usb_v1` and `usb_hcd_v1` exist because one component held the UART; they go once
  every component has its own channel (L-B).

### Test images

| Image | Serial holder | Log clients |
|---|---|---|
| wood-test | `log_server` | compositor, Wood, midi_synth, audio driver |
| compositor-test, window-events-test | compositor | none (the compositor's import is unlinked) |
| audio-test | audio driver | none (unlinked) |
| usb-core-test, usb-hid-test | `usb_core` / `input_trace` | none |
| production (`kernel/kickstart/image.toml`) | shell | none (the compositor's import is unlinked) |

### Gaps

- **Kernel: none.**
- **Kickstart/Meddle:** no client-to-exporter signal in `channel` wiring (worked
  around with `flush`); `MAX_AREAS` = 8; exporters do not learn clients' names;
  `serial = true` should be checked to name exactly one component.
- **image-build:** the top-level `interfaces` key (L4) was added for unlinked imports.
- **Out of scope:** the nucleus and Kickstart print nothing on the board's UART; a
  kernel panic on the board is silent.

## Prior art

- **Genode LOG service and `terminal_log`.** Each LOG session carries a label the
  parent sets (`init -> app`), and the server prefixes every line with it, so a client
  cannot claim another's name; `terminal_log` serves LOG and writes the labelled lines
  into a Terminal session, the split of `log_server` over `uart-pl011`.
  <https://genode.org/documentation/genode-foundations/>,
  <https://github.com/genodelabs/genode/tree/master/repos/os/src/server/terminal_log>.
- **seL4 sDDF / LionsOS.** Clients have shared-memory queues to transmit and receive
  virtualisers in front of one driver; the transmit side needs a policy when clients
  exceed the device's capacity, and the receive side routes input to one client (L3,
  L7, L11). <https://github.com/au-ts/sddf>, <https://arxiv.org/abs/2501.06234>. The
  older CAmkES `SerialServer` gives each client a buffer and colours its output:
  <https://github.com/seL4/global-components>.
- **Fuchsia debuglog and syslog.** `zx_debuglog_write` is a kernel log every early
  process can write (records cut to fit); components log through `LogSink` to the
  Archivist, which attributes records and counts drops instead of blocking writers.
  <https://fuchsia.dev/reference/syscalls/debuglog_write>,
  <https://fuchsia.dev/fuchsia-src/concepts/components/diagnostics/logs/>.
- **L4Re `L4Re::Log`.** A log capability (served by Moe) that serialises output and
  tags what is sent through a given log object: the tag belongs to the capability.
  <https://os.inf.tu-dresden.de/L4Re/doc/group__api__l4re__log.html>.
- **NOVA/Hedron** have no log server of their own; the roottask or VMM owns the UART.

The common thread: one owner of the device, per-client channels, identity attached by
the server where hostile clients matter, and writers that drop with a count instead of
blocking.
