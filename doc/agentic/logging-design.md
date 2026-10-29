# Logging design (several components, one PL011)

Status: decisions L1 to L10 answered on 2026-10-28 and L11 and L12 on 2026-10-29 (section
Decisions). Stage L-A (the
log server in wood-test with the compositor, Wood, the MIDI synthesiser and the audio
driver as clients, and Wood's music summary) is built and checked in QEMU; the board
check is pending. In L-B the three USB components became clients too (section USB log
volume). Stage L-D, panic and fault reports, was taken up on 2026-10-29 ("keep 32k
stacks, add panic reporting through the serial console too"); the `PANICKED:` and
`FAULT:` lines are built and checked in QEMU (section Panics and faults). No logging
question is open
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
  32 lines). A component's panic and its Threads' faults go through its own log channel,
  best effort, besides the semihosting console: stage L-D, in scope since the
  maintainer's 2026-10-29 request ("add panic reporting through the serial console
  too"; section Panics and faults). The nucleus and Kickstart stay semihosting-only.
  Not chosen: a polled direct-UART fallback in components (breaks the one-owner rule);
  a kernel log object such as Fuchsia's debuglog (minimal kernel API).
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
  (b); an output-only serial port (c).
- **L12. Serial console at boot: c, negotiate the rate at boot as chainboot does;
  Kickstart prints its early lines on the UART.** Answered 2026-10-29. Goal: an image
  booted from the SD card (no chainboot) still has a working serial console.
  Kickstart sets the PL011 up itself first thing at EL2 (MMU off): GPIO 14/15 to ALT0
  (the maintainer's `config.txt` has `dtoverlay=disable-bt`, Kickstart routes the pins
  anyway, as chainboot does), the UART clock from the mailbox, 115 200 8N1 with
  FIFOs. It then offers a faster console for 1.5 s: the console beacon (`^B^B^B`),
  answered by chainofcommand with chainboot's rate request for `--load-baud`, the
  same answers, a switch with `SWITCH_SETTLE`, and a confirmation at the new rate
  (`^F^F^F` back from the host) or a fall back to 115 200 (protocol in
  `libs/chainboot-protocol`, "The console negotiation"). Nobody answering costs the
  1.5 s and leaves 115 200. Kickstart's early boot lines go to the UART as well as
  semihosting (`kernel/kickstart/src/early_console.rs`), until just before the MMU
  goes on and Kickstart drops to EL1, where the UART is not mapped: the
  `uart-pl011` driver is the one owner from its start and keeps the divisors, so the
  negotiated rate carries to the log server. This narrows L8's "Kickstart stays
  semihosting-only" to Kickstart's EL1 part.
  - **Chainboot detection: a handoff.** Chainboot jumps to the kernel with
    `x1 = BAUDKEEP` (ASCII, little-endian) and `x2` = the rate; `libboot` keeps both
    (`loader_registers`), and Kickstart then uses the UART as chainboot left it and
    skips the negotiation (the earlier decision: the kernel stays at the rate the boot
    loader set). The firmware's stub and QEMU leave both zero. Not chosen: sniffing
    PL011 UARTCR.UARTEN and the divisors, which cannot tell chainboot's negotiated rate
    from the firmware's `enable_uart=1` at 115 200, which must still negotiate.
  - **Host side:** chainofcommand reacts to the console beacon in every mode, asking
    for `--load-baud` (no separate option); `--console` sends no kernel, and
    `just console` runs it that way for a board booted from the SD card.
  - Not chosen: (a) keep the rate the UART has, or default to 115 200 (a board booted
    from the SD card is stuck at 115 200, or at whatever the firmware left); (b) a
    fixed rate chosen at build time (the host must match it by hand, and a rate the
    line cannot carry loses the console with no way back).

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
8. Panics and faults reach the UART when the log path works.
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
- **USB components (L-B):** `usb` (`drivers/usb-hcd-dwc2`), `usb_core` (`drivers/usb`)
  and `usb_hid` (`drivers/usb-hid`) import `log_v1`; their `note`/`Trace` lines reach
  the UART, and the controller's backlog relay through `usb_core` finds nothing left.

### USB log volume

The maintainer: "all three, not too spammy though - i don't want usb debug output to
overwhelm the timings". The policy is a boot-time trail and a quiet steady state:

- **Traced once per event:** the controller's power-domain answer, core id and core
  reset (or the bring-up step that timed out), root port power and connect time
  (`usb-hcd-dwc2: …`); the root port's first status and connect, each port reset's
  outcome with the port register; each control transfer of an enumeration with its
  setup packet and bytes moved or its failure decoded (stage, channel events), and
  the device found (speed, address, class, ids); hub descriptors, Status Change
  pipes, each interrupt pipe scheduled, each HID claim with its pipe parameters,
  the device served, devices gone (`usb-core: …`, `usb-hid: …`).
- **Never traced:** a transfer, poll, report or interrupt that went as expected;
  ports at rest; key presses.
- **Back-off, the existing `is_power_of_two()` style:** whatever repeats is traced the
  1st, 2nd, 4th, 8th… time with its running count: poll failures, polls and control
  transfers that held the channel past 20 ms, channels that did not halt, failed hub
  requests (port status, set/clear feature), failed HID claims and set-ups (retried
  every 250 ms), and the waits for a root device and for a keyboard or mouse to claim
  (at 1, 2, 4, 8… seconds).
- **Levels:** `Trace` and `note` send everything at Info; there is no Debug-only line,
  since nothing per transfer is traced at all.

In `just test-wood` (a keyboard behind a hub) the three components write 38 lines in a
12.5 s run: 33 in the first 0.76 s (bring-up and the enumeration of the hub and the
keyboard), then only the back-off lines (`no Mouse yet` at 1, 2, 4 and 8 s, one slow
poll): 5 lines in the remaining 11.7 s, about 0.4 a second and falling, none for the
keys.

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

### Panics and faults (L-D)

A component's last words go to the UART through its own `log_v1` channel. Seen on
the board: wood-test lost its USB keyboard because `usb-core`'s Thread overflowed its
stack; the kernel's fault message went to semihosting only and the Thread just
stopped.

**Panics.** The EL0 panic handler (`userspace/runtime`) prints `EL0 PANICKED: …` on the
semihosting console as before, then calls a reporter a linked library registered with
`libuser::set_panic_reporter`, then exits QEMU (`qemu`) or halts. The runtime cannot
depend on `liblog_client` (the client depends on the runtime), so the hook is one
function pointer: `liblog_client` registers its reporter when it finds the component's
channel (the first line sent, or a fault). The reporter sends
`PANICKED: <component>: <message> at <file>:<line>` at level Error. Safeguards:

- **Never blocks unbounded:** `send` never waits; the line is retried 4 times 5 ms
  apart while another Thread holds the channel's try-lock, then the Thread gives the
  server up to 100 ms (5 ms sleeps on the channel's `EventCount`, which the server
  never advances) to take every buffer back, then 50 ms for the UART driver. Without a
  sleep key it spins on the clock for the same times.
- **Re-entrancy:** a flag set by the first panic; a panic while reporting (a message
  whose formatting panics, a logger left broken) or a second Thread's panic halts
  without a report (and prints `EL0 PANICKED again, not reported` under QEMU).
- **Inside the logger:** a panic in `send` leaves the try-lock held, so the report
  finds it busy and is dropped; it never touches the channel's state.
- **Small stack:** the line is formatted straight into a channel buffer (`LineWriter`,
  256 bytes, cut beyond); no extra buffer.
- **No log channel:** no reporter is registered; the panic behaves as before.

Under QEMU the panic still ends the run with `exit_failure`, after the bounded wait.

**Faults.** The kernel delivers a fault (data or instruction abort, alignment fault,
undefined instruction, a failed Return…) by making the faulting Thread call the
Invocation at its `AddressSpace`'s `KeySlot::FAULT_HANDLER`
([Thread, Fault delivery](../object_types/core/thread.md#fault-delivery)); with no
handler the Thread is parked as `Faulted` and the nucleus prints on semihosting
only. Kickstart installed no handler, so every component fault was silent on the board.
No kernel change was needed (option a/b of the request: an existing userspace path):

- `liblog_client` exports the handler, `vesper_fault_entry`
  (`libdevice_driver::FAULT_HANDLER_EXPORT`, a `ppc_export!` whose `.exports` record is
  kept in every image that links the crate). Kickstart (`kernel/kickstart/src/drivers.rs`)
  creates an Invocation of it at the component's `FAULT_HANDLER` slot, on a one-stack
  pool of 16 KiB in the component's stack region, for every component whose image has
  the export. Driver tables now fill from slot 17 (`FIRST_DRIVER_SLOT`), past the
  well-known slots up to `FAULT_HANDLER`.
- The handler runs on the faulting Thread, on that stack (so a Thread that overflowed
  its stack still has one), logs the line and returns `FaultAction::Terminate`: the
  Thread is parked as `Faulted`, as an unhandled fault was.
- The init record (`DriverInit`) carries the component's manifest name and each
  Thread's stack (`thread_stacks`, bottom and top), so the line names both;
  `MAX_IMPORTS` went from 32 to 28 to keep the record in its page (Wood, the largest
  importer, has 17).
- The line, built by `log_server::report::FaultReport` (host-tested):
  `FAULT: <component> thread <n>: <cause> at pc 0x…, far 0x…, sp 0x…, esr 0x…`. A data
  abort with a translation fault whose address lies in the stack region
  (`STACK_REGION`, 2 MiB, every non-stack page unmapped) is `stack overflow (guard
  page)`; the Thread is the one whose guard page holds the address, else the one whose
  stack holds SP; SP on none of them is `in an export call` (a client's call running
  on the export pool). Other causes come from `ESR_EL1.EC`.
- Under QEMU the handler prints the line on semihosting too; the kernel's own `⚠️
  fault` line stays.

What the maintainer sees on the board, for example:

```
[     3.141592] E FAULT: usb_core thread 0: stack overflow (guard page) at pc 0x40a1c4, far 0x30000ff0, sp 0x30000ff0, esr 0x92000047
[     5.000000] E PANICKED: compositor: index out of bounds: the len is 4 but the index is 4 at userspace/gui/compositor/src/input.rs:120
```

Limits: a frame larger than the 4 KiB guard can jump over it into the stack below and
corrupt it silently (no stack probes on this target); a fault inside the fault handler,
or a second fault while the handler runs, stays unhandled and silent; a component that
does not link `liblog_client` gets no handler. `just test-log` checks both lines (a
probe whose second Thread overflows its stack and whose first Thread panics).

### Interactions and limits

- **Kickstart limits.** `MAX_AREAS` (8 data areas per exporter) caps the log server at 8
  clients; wood-test uses 7 (with the USB components). Migrating the rest needs it raised
  (an `ExportedArea` is 32 bytes of the init page). Each channel takes 3 pages of the
  2 MiB data window shared by all pairs.
- **PL011 interrupt load at high baud.** At 1.5 Mbaud an 8-byte transmit burst drains
  in 53 µs: up to about 19 000 transmit interrupts per second while the log is busy;
  worth measuring before trusting timing measured at that rate. The FIFO level
  (`IFLS`) is the knob.
- **Stack depth.** `usb-core`'s Thread overflowed the 16 KiB stack Kickstart gave
  every Thread once it traced to the log channel under QEMU (the hub loop with an
  enumeration inlined takes 7.7 KiB, the semihosting `println!` buffer 4 KiB): driver
  Threads now get 32 KiB (`STACK_PAGES` in `kernel/kickstart/src/drivers.rs`; approved
  by the maintainer on 2026-10-29: "keep 32k stacks"), and
  `libserial-trace` prints to semihosting, sends to the serial channel and keeps
  backlog lines in separate non-inlined functions, so a trace line's stack is the
  larger of those paths, not their sum. Tracing from deep call chains must still keep
  frames small. On the board an overflow was silent until L-D: the fault report
  (section Panics and faults) now names it.
- **libserial-trace relays.** The backlog and the `trace(octet&)` exports of `input_v1`,
  `usb_v1` and `usb_hcd_v1` exist because one component held the UART; they go once
  every component has its own channel (L-B).

### Test images

| Image | Serial holder | Log clients |
|---|---|---|
| wood-test | `log_server` | compositor, Wood, midi_synth, audio driver, usb, usb_core, usb_hid |
| compositor-test, window-events-test | compositor | none (the compositor's and the USB components' imports are unlinked) |
| audio-test | audio driver | none (unlinked) |
| usb-core-test, usb-hid-test, usb-test | `usb_core` / `input_trace` / none | none (the USB components' imports are unlinked) |
| log-test | `log_server` | `log_probe` (overflows a stack, panics) |
| production (`kernel/kickstart/image.toml`) | shell | none (the compositor's and the USB components' imports are unlinked) |

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
