# USB implementation plan

Companion to [usb-design.md](usb-design.md), which holds the architecture and
interfaces. [usb-decisions.md](usb-decisions.md) holds the open decisions. This file
holds only open work. A finished item leaves it as a one-line marker in
[usb-completed.md](usb-completed.md). Mark an item `- [x]` only after the command
listed under Verification has passed for it.

Done: USB0 (design, interfaces), USB1 (protocol crate), USB2 (DWC2 root port and EP0), most of USB3 (the hub driver and `usb-core`).
See [usb-completed.md](usb-completed.md).

## USB2 follow-ups

- [ ] Serve bulk pipes (`pipe_v1.configure` already carries their kind and packet
  size). Control and interrupt-IN pipes are served, see [usb-completed.md](usb-completed.md).
- [ ] Validate DMA and cache maintenance on a real Raspberry Pi 3. QEMU does not
  model caches, and the bus-address alias from the node's `dma-ranges` is
  unchecked on silicon.

## USB3: hub and downstream devices

Done: the hub module, `usb-core` enumerating the root hub and the devices behind it, and
the serial trace; see [usb-completed.md](usb-completed.md).

- [ ] Verify split transactions on a Raspberry Pi 3: boot the `usb-core-test` image
  (`just boot-usb-core-test`, or copy `just build-usb-core-test-hw`'s image) with a low- or full-speed device on the LAN9514 and read
  `usb-core`'s trace on the UART. QEMU's hub is full speed and its controller has no
  transaction translator, so nothing here runs a split end to end.
- [ ] `usb_v1.control` on a claim: forward the claimant's stage to the device's control
  pipe. It needs the core's pipes shared with the exports' Threads, which today live on
  the enumeration Thread; a valid claim raises `not_permitted` until then.
- [ ] Teardown of in-flight transfers when a device is unplugged, and nested hubs
  (a device behind the hub that is a hub itself is traced and left alone).
- [ ] Bring `usb-core` and the controller into the Raspberry Pi 3 boot image
  (`kernel/kickstart/image.toml`) with the shell, which holds the serial channel the
  trace uses; only one program gets a serial channel today.
- [ ] Verify on a real Raspberry Pi 3 once USB2 is validated there.

## USB4: HID input

Needs decision 1. The input event contract is decided ([input-design.md](input-design.md)); the record library is `libs/input-event`.

- [ ] Verify interrupt-IN on a Raspberry Pi 3 and finish its hardware details: the
  odd/even frame placement (`HCCHAR.OddFrm`), the periodic transmit FIFO (`HPTXFSIZ`)
  that split interrupt polls need, and the split poll itself, which QEMU's controller
  cannot run (no transaction translator). QEMU runs the unsplit poll end to end (see
  [usb-completed.md](usb-completed.md)).
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
- `just test-usb-core`: `usb-core` enumerating the hub and a keyboard behind it in QEMU.
- `just test-meddle`: `DataArea` byte access (`libs/meddle-runtime`).
- `just test-device`, `just test-uart`, `just test-driver`, `just test-mailbox`: regression for shared
  changes.
- `just fmt-check`.

## Related

- [usb-decisions.md](usb-decisions.md): open USB decisions.
- [usb-design.md](usb-design.md): architecture, interfaces and the RPi4 follow-up.
- [input-design.md](input-design.md): the input side that consumes these events.
