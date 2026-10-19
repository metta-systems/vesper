# USB implementation plan

Companion to [usb-design.md](usb-design.md), which holds the architecture and
interfaces. [usb-decisions.md](usb-decisions.md) holds the open decisions. This file
holds only open work. A finished item leaves it as a one-line marker in
[usb-completed.md](usb-completed.md). Mark an item `- [x]` only after the command
listed under Verification has passed for it.

Done: USB0 (design, interfaces), USB1 (protocol crate), USB2 (DWC2 root port and EP0).
See [usb-completed.md](usb-completed.md).

## USB2 follow-ups

- [ ] Serve the rest of `usb_hcd_v1`: interrupt and bulk pipes (USB4; they need an `interval_ms` for interrupt pipes, which
  no longer fits `open_pipe`'s six words: a `pipe_v1` function sets it) and split transactions behind `set_route` (USB3).
  Control pipes are served, see [usb-completed.md](usb-completed.md).
- [ ] Validate DMA and cache maintenance on a real Raspberry Pi 3. QEMU does not
  model caches, and the bus-address alias from the node's `dma-ranges` is
  unchecked on silicon.

## USB3: hub and downstream devices

- [ ] `usbd` component in `drivers/usb`, serving `usb_v1`. Nothing exists yet
  beyond the interface.
- [ ] Hub logic in a `hub` module of `vesper-usb-protocol`, inside `usbd` (decision
  recorded in the design): hub descriptor parse, class requests (GET_HUB_DESCRIPTOR,
  GET_PORT_STATUS, SET_PORT_FEATURE, CLEAR_PORT_FEATURE), port status decode and the
  per-port state machine (power, debounce, reset, speed, teardown). Host-tested.
  `usbd` polls GET_PORT_STATUS over control until interrupt pipes exist (USB4).
- [ ] Enumerate devices behind the LAN9514 hub with split transactions, using
  `usb_hcd_v1.set_route`. Low- and full-speed devices need this.
- [ ] Port-change polling and hotplug, including teardown of in-flight transfers.
- [ ] Verify with a keyboard or mouse attached to the hub in QEMU, and with a real
  Raspberry Pi 3 once USB2 is validated there.

## USB4: HID input

Needs decision 1. The input event contract is decided ([input-design.md](input-design.md)); the record library is `libs/input-event`.

- [ ] Interrupt-IN over an io-channel. The HCD fills IO_Recs and advances the
  client's EventCount. See [usb-design.md](usb-design.md#control-and-data-planes).
- [ ] `hid` component in `drivers/usb-hid`: claim the boot keyboard or mouse,
  SET_PROTOCOL and SET_IDLE, and decode reports with `hid_boot`.
- [ ] Rework `input_v1` in place: `descriptor` and `snapshot` functions, and remove
  `set_leds` (output goes through the transmit channel, no interim Call). Publish input
  events (the record in [input-design.md](input-design.md)) over an io-channel.

## USB5: keyboard behaviour

- [ ] Typematic autorepeat in software.
- [ ] LED output reports from events on the input transmit channel. Needs the
  transmit channel (no interim Call).

## Verification

- `just test-usb-host`: protocol and DWC2 host tests.
- `just clippy-usb-host`: host lint for the same crates.
- `just test-usb`: end-to-end root hub enumeration and pipe rules through `usb_hcd_v1` in QEMU.
- `just test-meddle`: `DataArea` byte access (`libs/meddle-runtime`).
- `just test-device`, `just test-uart`, `just test-driver`, `just test-mailbox`: regression for shared
  changes.
- `just fmt-check`.

## Related

- [usb-decisions.md](usb-decisions.md): open USB decisions.
- [usb-design.md](usb-design.md): architecture, interfaces and the RPi4 follow-up.
- [input-design.md](input-design.md): the input side that consumes these events.
