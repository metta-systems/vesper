# USB implementation plan

Companion to [usb-design.md](usb-design.md), which holds the architecture, the
interfaces and the open decisions. [usb-decisions.md](usb-decisions.md) holds the
open decisions. This file holds the tasks and their status.
Checked items are verified by the command listed under them. 

## Phases

- [x] **P0: design and interfaces.** `usb-design.md` written. Interfaces drafted
  and parsed by `meddler check`: `usb_hcd_v1` (`drivers/usb-hcd-dwc2/interfaces`),
  `usb_v1` (`drivers/usb/interfaces`), `input_v1` (`drivers/usb-hid/interfaces`).
- [x] **P1: pure protocol logic.** `drivers/usb/protocol` (`vesper-usb-protocol`):
  setup packets, descriptor walker, enumeration state machine, HID boot reports.
  No I/O. Verify: `just test-usb-host` (36 tests), `just clippy-usb-host`.
- [x] **P2: DWC2 root port and EP0.** `drivers/usb-hcd-dwc2`: register map on
  `bitbybit` fields and a `derive_mmio` block, the control-stage plan, and an EL0
  component. Transfers run by DMA through a driver-owned page, cleaned and
  invalidated around each transfer. The root hub is enumerated. Verify:
  `just test-usb` (QEMU raspi3b with a hub on the root port), `just test-usb-host`.
  - Completion is polled, not interrupt-driven.
  - Packet data uses DMA only, because QEMU's DWC2 model does not implement
    slave-mode FIFO writes.
  - Only the root device is enumerated.

## P2 follow-ups

- [ ] Interrupt-driven completion: an IRQ Notification and a second Thread, as in
  the PL011 driver. Polling is fine for bring-up.
- [ ] Serve `usb_hcd_v1`: a generated Meddle server in `drivers/usb-hcd-dwc2`, so
  `usbd` can call it. Only the interface exists; nothing serves it yet.
- [ ] Validate DMA and cache maintenance on a real Raspberry Pi 3. QEMU does not
  model caches, and the bus-address alias from the node's `dma-ranges` is
  unchecked on silicon.

## P3: hub and downstream devices

- [ ] `usbd` component in `drivers/usb`, serving `usb_v1`. Nothing exists yet
  beyond the interface.
- [ ] Hub class requests: GET_HUB_DESCRIPTOR, GET_PORT_STATUS, SET_PORT_FEATURE
  and CLEAR_PORT_FEATURE.
- [ ] Enumerate devices behind the LAN9514 hub with split transactions, using
  `usb_hcd_v1.set_route`. Low- and full-speed devices need this.
- [ ] Port-change polling and hotplug, including teardown of in-flight transfers.
- [ ] Verify with a keyboard or mouse attached to the hub in QEMU, and with a real
  Raspberry Pi 3 once P2 is validated there.

## P4: HID input

- [ ] Interrupt-IN over an io-channel. The HCD fills IO_Recs and advances the
  client's EventCount. See [usb-design.md](usb-design.md#control-and-data-planes).
- [ ] `hid` component in `drivers/usb-hid`: claim the boot keyboard or mouse,
  SET_PROTOCOL and SET_IDLE, and decode reports with `hid_boot`.
- [ ] Serve `input_v1` (`set_leds`) and publish input events over an io-channel.
  The event format is open (usb-decisions.md, item 3).

## P5: keyboard behaviour

- [ ] Typematic autorepeat in software.
- [ ] LED output reports through `input_v1.set_leds`.

## Open decisions

Listed in [usb-decisions.md](usb-decisions.md), each with the phase that needs it.

## Shared changes made so far

- `libs/device-driver`: `MAX_DEVICE_PAGES` raised from 16 to 32 for the Pi 3 DWC2
  node (a 64 KiB window and a 4 KiB range).
- `libs/serial-channel`: `CONTROL_VA` moved from `0x3801_0000` to `0x3802_0000`,
  above the 32 register pages.
- `Justfile`: `usb-hcd-dwc2` added to `driver_components` and `user_components`.
  New recipes: `test-usb-host`, `clippy-usb-host`, `build-usb-test`, `test-usb`.
  The workspace list in `Cargo.toml` gained the three USB crates.

## Verification

- `just test-usb-host`: protocol and DWC2 host tests.
- `just clippy-usb-host`: host lint for the same crates.
- `just test-usb`: end-to-end root hub enumeration in QEMU.
- `just test-device`, `just test-uart`, `just test-driver`: regression for the
  shared changes above.
- `just fmt-check`.

## Related

- [usb-decisions.md](usb-decisions.md): open USB decisions.
- [usb-design.md](usb-design.md): architecture, interfaces and the
  RPi4 follow-up.
- [input-design.md](input-design.md): the input side that consumes these events.
