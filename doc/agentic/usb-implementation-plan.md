# USB implementation plan

Companion to [usb-design.md](usb-design.md), which holds the architecture and
interfaces. [usb-decisions.md](usb-decisions.md) holds the open decisions. This file
holds only open work. A finished item leaves it as a one-line marker in
[usb-completed.md](usb-completed.md). Mark an item `- [x]` only after the command
listed under Verification has passed for it.

Done: P-USB-0 (design, interfaces), P-USB-1 (protocol crate), P-USB-2 (DWC2
root port and EP0), P-USB-3 (the hub driver, nested hubs and `usb-core`),
P-USB-6 (xHCI on the Raspberry Pi 4) and P-USB-7 (DWC2 follow-ups). See
[usb-completed.md](usb-completed.md).

## P-USB-8: DWC2 follow-ups, the rest

- [ ] Serve bulk pipes (`pipe_v1.configure` already carries their kind and packet
  size). Control and interrupt-IN pipes are served, see [usb-completed.md](usb-completed.md).
- [ ] Validate DMA and cache maintenance on a real Raspberry Pi 3. QEMU does not
  model caches, and the bus-address alias from the node's `dma-ranges` is
  unchecked on silicon.

## P-USB-9: hub and downstream devices, the rest

- [ ] `Clear_TT_Buffer` to the hub after a failed split transaction, which can leave
  the translator's buffer busy (USB 2.0 §11.17.5).
- [ ] `usb_v1.control` on a claim: forward the claimant's stage to the device's control
  pipe. It needs the core's pipes shared with the exports' Threads, which today live on
  the enumeration Thread; a valid claim raises `not_permitted` until then.
- [ ] Teardown of in-flight transfers when a device is unplugged.
- [ ] The LAN9514's Ethernet function (hub port 1, address 2, high speed, no split)
  failed `SET_CONFIGURATION` on the board in `boot-wood-test`: the status stage (a
  zero-length IN) was retried for the whole 1 s budget and ended `halted xacterr`
  (`usb-hcd-dwc2: control to device 2 held the channel 1000 ms`), although it
  configured in earlier `boot-usb-core-test` runs. Not reproduced. A device must
  finish a no-data request's status stage within 50 ms (USB 2.0 §9.2.6.4). Found
  since (2026-10-29, see [usb-completed.md](usb-completed.md)): the driver unmasked
  NAK and halted the channel at the first NAK interrupt of a high-speed status
  stage while the controller, in DMA mode, was still retrying it; it now waits for
  the halt. The failure line now counts the runs that ended in NAK and in XactErr.
  To check on the board: whether it recurs, and which count dominates. Not chosen
  yet: cutting the status stage's retry budget to the spec's 50 ms.
- [ ] Verify the 2026-10-29 triage on the board (`just _boot-usb-hid-test`, then
  `just boot-wood-test`), in each topology: mouse straight on a LAN9514 port,
  keyboard (its own high-speed hub) on a LAN9514 port, mouse on the keyboard's
  hub. Expected: no `complete split ... restarted` lines at steady state (or few,
  with the last complete split 2 to 4 microframes after the start split), the
  keyboard's hub's first `GET_DESCRIPTOR` answering, and any failed enumeration
  followed by `disabled after the failed enumeration`. Open on hardware only: the
  databook's DMA-mode NAK retry for non-periodic channels and the `OddFrm`
  targeting are untested in QEMU, whose controller has no translator.
- [ ] Bring `usb-core` and the controller into the Raspberry Pi 3 boot image
  (`kernel/kickstart/image.toml`) with the shell, which holds the serial channel the
  trace uses; only one program gets a serial channel today.
- [ ] Verify on a real Raspberry Pi 3 once P-USB-8's DMA check has passed there.

## P-USB-4: HID input

Needs Q-USB-1. The input event contract is decided ([input-design.md](input-design.md)); the record library is `libs/input-event`.

- [ ] Interrupt-IN on the Raspberry Pi 3 works through split polls (see
  [usb-completed.md](usb-completed.md)); still open: the periodic transmit FIFO
  (`HPTXFSIZ`), polls and control transfers sharing the one host channel (a control
  transfer the device answers NAK for the whole retry budget still holds it for a
  second), and an unsplit interrupt device on the board. Multi-TT operation of a
  protocol-2 hub (alternate setting 1, USB 2.0 §11.23.1) is not selected; every
  split through such a hub shares one translator.
- [ ] `hid`: frames missed while `input-trace` has no buffer posted show as `Dropped`;
  a second keyboard or mouse (one of each is claimed), `usb_v1.control` instead of its
  own control pipe, and the keyboard's real `max_packet0` for its setup requests (it
  uses 8).

## P-USB-5: keyboard behaviour

- [ ] Typematic autorepeat in software.
- [ ] LED output reports from events on the input transmit channel. Needs the
  transmit channel (no interim Call).

## P-USB-10: SuperSpeed and the Raspberry Pi 5

Follows USB-10; the xHCI decisions are USB-7 to USB-10 ([usb-design.md](usb-design.md)).

- [ ] SuperSpeed root ports, and RP1's DWC3 xHCI controllers on the Raspberry Pi 5.

## Verification

- `just _test-usb-host`: protocol and DWC2 host tests.
- `just _clippy-usb-host`: host lint for the same crates.
- `just _test-usb`: end-to-end root hub enumeration and pipe rules through `usb_hcd_v1` in QEMU.
- `just _test-usb-core`: `usb-core` enumerating the hub and a keyboard behind it in QEMU.
- `just _test-meddle`: `DataArea` byte access (`libs/meddle-runtime`).
- `just _test-device`, `just _test-uart`, `just _test-driver`, `just _test-mailbox`: regression for shared
  changes.
- `just fmt-check`.

## Related

- [usb-decisions.md](usb-decisions.md): open USB decisions.
- [usb-design.md](usb-design.md): architecture, interfaces and the RPi4 xHCI.
- [input-design.md](input-design.md): the input side that consumes these events.
