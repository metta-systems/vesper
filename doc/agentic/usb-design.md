# USB stack design (Raspberry Pi 3, HID first)

Status: USB0–USB2 done (see [usb-completed.md](usb-completed.md)); USB3 onward is open
in [usb-implementation-plan.md](usb-implementation-plan.md). Interfaces are
prototypes and are edited in place. Open decisions are in
[usb-decisions.md](usb-decisions.md) and are not assumed.

## Implemented facts

- DMA strategy: DMA through a driver-owned page, cleaned and invalidated around
  each transfer. Slave mode is not used, because QEMU's raspi3b DWC2 model does not
  implement slave-mode FIFO writes: OUT and SETUP data are fetched from `HCDMA`, and
  FIFO reads return 0.
- The DMA page is a frame retyped from the driver's own memory (`DriverInit.memory`),
  mapped cacheable, with `Frame CleanData` before OUT transfers and
  `InvalidateData` before the CPU reads IN replies. The write-combining ceiling is
  refused for RAM untypeds, so the uncached route is not available.
- Delays use the generic timer's virtual counter, `CNTVCT_EL0`, because EL0 may not
  read the physical counter.
- Completion is interrupt driven. The component's own Thread is the interrupt
  Thread: it waits on the IRQ Notification, reads `HAINT`, clears the events of
  the transfer channel, accumulates them in an atomic while a transfer is armed,
  advances a `completions` EventCount and acknowledges the line. A transfer arms the
  channel, then sleeps on `completions` with the remaining deadline (500 ms), so a
  missing interrupt ends it with `transfer_failed`. Port events stay masked.
  `GINTSTS.CurMod` is 1 in host mode, and the component waits for that bit.
- Interrupt masks follow the databook's DMA mode (2026-10-29): a non-periodic run
  and every split phase unmask only `ChHltd` and `AHBErr`
  (`regs::halt_event_mask`) and read the reason from `HCINTn` at the halt. The
  controller retries a NAKed non-periodic transaction itself, so a NAK bit on a
  completed transfer is not a failure (`channel_outcome`); before, the driver
  halted the channel at the first NAK interrupt while the controller was still
  retrying. A poll of a root-port interrupt endpoint keeps NAK unmasked (a
  periodic NAK halts the channel anyway). `halt` leaves a channel that already
  halted alone instead of setting `ChEna` with `ChDis` on it.
- Every channel run is composed from scratch (`regs::channel_program`):
  `HCSPLTn` (zero without a route), `HCTSIZn` (bytes, packet count, PID),
  `HCDMAn`, then `HCCHARn` (address, endpoint, direction, type, packet size,
  EC/MC 1, `OddFrm` only for periodic runs) with `ChEna`, so no split state or
  `OddFrm` carries into the next run. Data toggles: SETUP DATA0, data stage
  from DATA1, status DATA1; interrupt pipes keep their toggle in `Poller`.
- Periodic split polls (USB 2.0 §11.18.4, §11.18.6): the start split goes in
  microframe 0 to 4 of a frame (`microframe::start_split_target`), and the
  complete splits 2, 3 and 4 microframes after it (`Y+1`..`Y+3`). The interrupt
  Thread issues them itself at the halt of the run before (`SplitEngine` in
  `main.rs`: after the start split's ACK, and after each NYET while the window
  lasts), aimed at the next microframe through `HFNUM` and `HCCHARn.OddFrm`,
  spinning at most a microframe when the next one is too early; the poller wakes
  only for the answer or a missed window (`split::Event::Missed`, the poll starts
  over). Before, the poller issued each complete split one Thread hop after the
  halt, as soon as it could, and a fixed four NYETs ended the window, whatever
  microframes they fell in. Non-periodic split control transfers have no window
  (§11.17.1): a NYET is followed by another complete split until the 1 s budget
  ends, never by a new start split.
- Failure traces (rate-limited 1st, 2nd, 4th, 8th...): a failed control transfer
  names the setup bytes, device, speed, route, stage, duration and the count of
  channel runs that ended with each of NAK, NYET, XactErr, STALL, babble,
  DataTglErr, FrmOvrun and AHBErr (`regs::EventCounts`, with the one that
  dominates); a failed poll adds the counts of that poll and of the pipe's
  failed polls; a restarted periodic complete split names the start split's
  microframe and the distance to its last complete split.
- Kickstart caps a node at `MAX_DEVICE_PAGES` (32). The serial channel's control
  page sits at `0x3802_0000`, above the register pages.
- `drivers/usb-hcd-dwc2` resets the root port and serves `usb_hcd_v1` for control
  pipes (generated crates `drivers/usb-hcd-dwc2/server` and `client`). It does not
  enumerate: a client resets the port, has the controller address the device
  (`address_device`, which sends `SET_ADDRESS` to address 0 and records the
  device's speed and translator, `drivers/usb-hcd-dwc2/src/devices.rs`), opens a
  pipe to that address and enumerates through `vesper_usb_protocol`.
  `usb-test-client` does so in `just _test-usb` and finds the hub (class 9) at
  address 1.
- A call runs on the caller's Thread, one at a time (a `Turnstile`). The client
  stages the setup packet and data stage in its data area; the component copies
  them to and from its DMA page (data stage at most 4032 bytes). `DataArea` has
  byte access for this (`libs/meddle-runtime`).
- Pipes are handles in a 16-entry table: index plus generation, never zero. The handle
  is the badge of the pipe's `pipe_v1` keys, so a closed pipe's keys fail even when its
  slot is reused. The data area of the client that opened the pipe
  still bounds the stage a control call may name. A pipe is opened to an endpoint
  address (bit 7 is the direction) and configured with `pipe_v1.configure` (kind,
  `max_packet`, and the polling interval of an interrupt pipe), again whenever a
  device's real control packet size is learned. A control pipe's `max_packet` is
  8, 16, 32 or 64; interrupt and bulk pipes raise `not_permitted`.
- `set_route` stores the route, and `control` on a routed pipe runs each packet as a
  split transaction: a start split, then complete splits until the hub has the
  result (`split::Sequence`). A high-speed pipe cannot be routed. The route is the
  nearest high-speed hub upstream of the device and that hub's port
  (`hub::translator_route`, USB 2.0 §11.14): a low-speed keyboard behind the
  high-speed hub inside the keyboard goes through that hub's translator, not the
  LAN9514's. Multi-TT hubs run single-TT (alternate setting 0, §11.23.1).
- `usb-core` disables a hub port whose device failed to enumerate
  (`ClearPortFeature(PORT_ENABLE)`, `hub::PortAction::Disable`, §11.5.1.4) and
  only then returns the address the device was to take to the pool. Before, a
  failed device stayed on an enabled port at address 0 or at the address it took
  before failing, while the pool handed that address to the next device: two
  devices answered the same tokens. A high-speed device's endpoint 0 is opened
  with 64-byte packets from the first read (§5.5.3).
- `drivers/usb/protocol` has the setup packet, descriptor walker, enumeration state
  machine and HID boot-report decoder. The state machine has no retry and no
  timeout; the caller supplies delays and port resets as steps. Keyboard decoding
  ignores roll-over reports. Not covered: hub descriptors, class requests beyond
  SET_CONFIGURATION, HID report descriptors (boot protocol only).

## Layers

Modelled on CSUD (<https://github.com/Chadderz121/csud>), split into components
that Kickstart wires statically:

```
usb_hcd_v1 (dwc2 HCD)  <- control/pipe calls ->  usb_v1 (usb-core: core + hub)  <- claim/control ->  hid
        ^                                                                                       |
        +------------- interrupt-IN reports over io-channel, bypassing usb-core ----------------------+
                                                                              hid -> input_v1 events (io-channel) -> shell
```

| Component | Directory | Exports | Imports |
|---|---|---|---|
| `dwc2` (HCD) | `drivers/usb-hcd-dwc2/` | `usb_hcd_v1` | none |
| `usb-core` (core, hub inside) | `drivers/usb/` | `usb_v1` | `usb_hcd_v1` |
| `hid` (keyboard, mouse) | `drivers/usb-hid/` | `input_v1` | `usb_v1`, `usb_hcd_v1` |

- The HCD does registers, DMA and the schedule. It owns the MMIO frame, the
  IRQ Notification and a DMA Untyped, and runs two threads as the PL011 driver
  does.
- `usb-core` runs enumeration, descriptors and hub polling. It is control-plane
  only.
- `hid` claims boot-protocol keyboards and mice, posts empty IO_Recs for
  interrupt-IN, and decodes boot reports with a pure library. It imports
  `usb_hcd_v1` as well, because it opens its own interrupt pipe (maintainer
  decision, 2026-10-19): `usb_v1.interrupt_pipe` tells it the device address,
  endpoint, speed, interval, packet size and route, so the reports arrive on its
  own channel. The controller opens a pipe for any importer to any endpoint, so
  a claim is a convention until the controller can check one.

## Control and data planes

- Control plane (Invocations): enumeration, control transfers, SET_PROTOCOL,
  SET_IDLE, LED output reports. Rare and RPC-like.
- Data plane (io-channel): interrupt-IN HID reports. Client posts empty IO_Recs;
  the HCD fills them on completion and advances the client's EventCount. With no
  empty IO_Rec posted, the HCD does not schedule the IN transfer, so backpressure
  is natural. The drop-and-count rule of `io-channel-design` remains a safety net.
- Interrupt-IN channel (maintainer decisions, 2026-10-19): Kickstart provisions the
  channel, from a declaration in the interface, once for each client of the controller
  (as it does for the control data area); the channel is per client, not per pipe, and
  each packet's value word carries the pipe's handle; while no empty buffer is posted
  the controller does not re-arm the pipe. A more universal way to provision shared
  channels is future work (see the Future work section of
  [capabilities-decisions.md](capabilities-decisions.md)).
- CPU time for the Threads that serve data pipes needs no scheduler of its own for HID
  (an 8 ms interval). A guaranteed share for isochronous pipes belongs to a separate
  isochronous scheduler, parked until audio needs it (same Future work section).
- Data areas: control transfers stage the setup packet and data stage in the
  client's `data` area (`octet&`), following the mailbox convention.

## Interfaces (drafted)

- **Hub location (maintainer decision, 2026-10-18).** The hub driver is inside `usb-core`,
  not a separate component. Its pure logic (hub descriptor, class requests, port
  status, per-port state machine) goes in a `hub` module of `vesper-usb-protocol`,
  so it stays host-testable and could be extracted later.
- **Pipe authority (maintainer decisions, 2026-10-18 and 2026-10-19).** A pipe is a set of
  Invocation keys issued by the host controller, not a number checked against caller
  identity. `pipe_v1` is a `local` interface of its own, one key per function, all
  badged with the pipe's handle; `usb_hcd_v1.open_pipe` installs the keys in a key table
  the caller lends and returns their values in the data area. Holding a key is the right
  to use the pipe, so `usb-core` can hand a pipe to `hid` by installing a narrowed copy in
  `hid`'s table. The mechanism is in the
  [contract](capabilities-contract.md#component-interfaces-and-imports).
- `drivers/usb-hcd-dwc2/interfaces/usb_hcd_v1.if`: `reset_port`, `port_status`,
  `address_device`, `release_device`, `mark_hub`, `open_pipe`, `trace`. Parameters
  are controller-neutral. The controller assigns addresses (X2a, interface answered
  2026-11-17): `address_device(hub_address, port, speed)` names where the device is
  (`hub_address` 0 for a root port) and the controller works out the route from the
  hub's own record; `release_device(address)` frees the address (xHCI Disable
  Slot) and closes the device's pipes, called on teardown and after a failed
  enumeration once the port is disabled; `mark_hub(address, ports, think_time,
  multi_tt)` follows the hub descriptor (xHCI Configure Endpoint on the slot, DWC2
  nothing); xHCI refuses to address a device behind a hub not yet marked. A device
  that does not take its address raises `transfer_failed { status }` (DWC2 channel
  status, xHCI completion code), distinct from `no_device` for an empty root port
  (maintainer review, 2026-11-17). `open_pipe` takes the first slot of the lent table, the device address,
  the endpoint address and the speed, and installs the keys of `pipe_v1`.
- `drivers/usb-hcd-dwc2/interfaces/pipe_v1.if`: `configure`, `set_route`, `close_pipe`, `clear_halt`,
  `control`, the functions of one open pipe; the split-transaction route is set
  separately with `set_route`, which xHCI accepts and ignores (its slot carries the
  translator).
- `drivers/usb/interfaces/usb_v1.if`: `claim_interface` (raises `not_ready` while no
  matching interface is attached, so the claimant retries; maintainer decision,
  2026-10-19), `release`, `interrupt_pipe` (the parameters for `open_pipe`), `control`.
- `drivers/usb-hid/interfaces/input_v1.if`: `descriptor` and `snapshot` (the snapshot is
  the records that rebuild a device's state, ending with a `Sync`; no `set_leds`, output
  waits for the transmit channel). Events travel over an io-channel (`channel 1`) whose
  records are defined in [input-design.md](input-design.md); the client posts empty
  buffers and each comes back holding one frame.
- `imports.if` files: `usb-core` uses `usb_hcd_v1` `via device compatible
  "brcm,bcm2708-usb" or component xhci`: the DWC2 node on the Raspberry Pi 3, else the
  manifest's `xhci` driver on the Raspberry Pi 4, whose VL805 node has no compatible
  string (maintainer decision, 2026-11-17: a `uses` line may name fallback providers
  after `or`, Kickstart takes the first that finds a loaded exporter). `usb-hid` is comment-only
  until its device tree link is decided.

## Constraints from Meddle

- `octet&` requires a `local` interface with `data <bytes>;`.
- At most six argument words per call, and one word of return value.
- Exceptions carry at most one word.

## Open decisions

Open decisions are in [usb-decisions.md](usb-decisions.md). The decisions answered
during USB2 are recorded in the status section above.

## Platform facts

- RPi3 has no xHCI. The BCM2837 DWC2 root port feeds an SMSC LAN9514 that
  provides four USB 2.0 ports and Ethernet, so every downstream port sits behind
  a real hub. Hub support and split transactions are needed even for a single
  keyboard. Sources: <https://blog.ittraining.com.tw/2017/06/raspberry-pi-3-ethernet-chip-lan9514.html>,
  <https://github.com/raspberrypi/linux/issues/2021>.
- RPi4 uses a VL805 xHCI on PCIe. See [RPi4 xHCI](#rpi4-xhci).

## RPi4 xHCI

Hardware: USB-A ports sit on a VL805 xHCI (`1106:3483`) behind the BCM2711 PCIe
controller (`pcie0`, `brcm,bcm2711-pcie` at 0x7d500000). The Pi 4 DMA window is
capped at the first 3 GB of RAM, so xHCI buffers must come from below that. The
USB 2 ports sit behind a VIA hub (`2109:3431`) on the VL805's USB 2 root port, so
a keyboard there goes through `usb-core`'s hub path, as on the RPi3.

Maintainer decisions (2026-11-15, all as recommended):

- **X1a:** the xHCI driver itself sends the firmware's VL805 notify before it
  touches the controller, through `mailbox_v1.property` with the tag allowed.
  It builds the PCI address from the function `pci_v1.claim` reports.
- **X2a:** `usb_hcd_v1` is edited in place for device addressing: the controller
  assigns addresses (`address_device`: xHCI Enable Slot and Address Device, DWC2
  `SET_ADDRESS`), a hub is marked as one so xHCI can fill its slot's hub fields,
  `usb-core` stops sending `SET_ADDRESS`, and `controller_failed.step` is worded
  per controller.
- **X3a:** slices: (1) `drivers/usb-hcd-xhci` brings the controller up (notify,
  halt and reset, DCBAA and scratchpad, command ring, one event ring on MSI
  through `pci_v1`, a No Op command round trip) and serves `port_status` and
  `reset_port` on the USB 2 root ports, in board-test on the Raspberry Pi 4
  node in place of `pci-probe`; (2) X2a and control transfers, so `usb-core`
  enumerates the hub; (3) interrupt-IN, so `usb-hid` gets a keyboard.
- **X4a:** USB 2 root ports only at first; SuperSpeed ports and RP1's DWC3
  controllers later.

Bring-up, in order:

1. **PCIe.** Done by `pcie-brcmstb`; the xHCI driver claims the function and
   its BAR through `pci_v1`.
2. **VL805 firmware.** After a PCIe reset the VL805 loads firmware from an EEPROM
   if one is present, otherwise from VideoCore. Pi 4B 1.4 dropped the SPI EEPROM,
   so VideoCore must load it there. The request is tag
   `RPI_FIRMWARE_NOTIFY_XHCI_RESET` (0x0003_0058) with one 32-bit value, the
   function's address `bus << 20 | device << 15 | function << 12` (0x0010_0000
   for 01:00.0); Linux then waits 200 µs to 1 ms for the VL805 to start
   (`drivers/reset/reset-raspberrypi.c` and
   `include/soc/bcm2835/raspberrypi-firmware.h` in the rpi-6.12.y kernel). Linux
   sends it from the xHCI driver's probe, through the node's
   `resets = <&reset RASPBERRYPI_FIRMWARE_RESET_ID_USB>`, before the controller
   is touched (`drivers/usb/host/xhci-pci.c`). Without the load the xHCI can hang.
3. **xHCI core.** Wait for CNR to clear in USBSTS; set MaxSlotsEn in CONFIG;
   point DCBAAP at the device context base array (plus scratchpad buffers if
   HCSPARAMS2 asks for them); set up the command ring and event ring
   (ERSTBA, ERSTSZ, ERDP); set Run/Stop. Smoke test with a no-op command and its
   completion event
   ([Agilex xHCI programming model](https://intel.com/content/www/us/en/docs/programmable/814346/24-3/programming-model-56434.html),
   [Linux xhci.h history](https://git.linaro.org/plugins/gitiles/kernel/linux-linaro-stable.git/+/33b2831ac870d50cc8e01c317b07fb1e69c13fe1%5E%21/drivers/usb)).
4. **Interrupts.** The VL805's MSI reaches the driver's Notification through
   `pcie-brcmstb`, which `pci_v1.claim` sets up.
5. **Enumeration.** Per root port: reset it (USB 2 ports need this before they are
   enabled), then Enable Slot, Address Device, GET_DESCRIPTOR, Configure Endpoint.
   HID interrupt-IN then runs on the normal ring path.

Linux's VL805 quirks (`xhci_pci_quirks`): 40-bit DMA, broken dequeue cycle
state in endpoint contexts, TRB overfetch, no dequeue moves onto a link TRB,
and VIA SuperSpeed bulk-out and hub TT bugs. The VL805 raises no interrupt
after `ERDP` is written high word first; the driver writes it low word first, as
xHCI §5.1 has it.
[doc/usb.md](../usb.md#via-vl805-no-interrupts-after-erdp-is-written-high-word-first)
has the evidence and how the TRB read-ahead bug Linux works around differs.

Interrupt-IN on xHCI keeps `pipe_v1`'s rule that a pipe is polled only while
its client has an empty buffer posted: one Normal TRB is out per pipe, on a
64-TRB ring that shares a page with the report buffer (the ring's tail is zero
memory, so the VL805's TRB overfetch reads nothing live), and the controller
does the polling. After 2 s without an MSI while a TRB is out, a watchdog
drains any events no MSI brought in and traces that it found some. A client
posts
buffers without a signal, so the service Thread looks at the channels every
2 ms while a pipe waits for one. A stall waits for `clear_halt`; other failures
are recovered at once (Reset Endpoint, then Set TR Dequeue Pointer to the
enqueue pointer, which never sits on the link TRB).

Isochronous transfers can wait. The DMA strategy (answered in USB2, see the
status section) also applies to xHCI rings.

## Prior art and candidate crates

Survey from web search on 2026-10-17, not a commitment. Nothing has been checked
for licence, maintenance activity or no_std build status beyond the listing.

- Host side, no_std Rust:
  - [crab-usb](https://docs.rs/crate/crab-usb/0.9.9): async host driver, xHCI only, per search results.
  - [cotton-usb-host](https://docs.rs/cotton-usb-host): no_std, no-alloc host stack, targets the RP2040 controller, not DWC2.
  - [usb-hal](https://lib.rs/crates/usb-hal): early; its host part is listed as unimplemented.
  - [embassy-usb](https://docs.rs/crate/embassy-usb/latest) lists `embassy-usb-synopsys-otg` for the DWC2 family. Whether it supports host mode was not confirmed.
- Device side, not useful for a host HID path:
  - [usb-device](https://github.com/rust-embedded-community/usb-device) and [usb-core-hid](https://docs.rs/usb-core-hid) (latest 0.7.0 listed as Nov 2023).
  - [usb-core-human-device-interface](https://docs.rs/usb-core-human-device-interface) is an alternative HID class crate.

No candidate found covers DWC2 host mode and the HID host class in no_std Rust.

## Related

- `doc/agentic/capabilities-contract.md`: PPC, xbufs and EventCounts.
- `drivers/mailbox/interfaces/mailbox_v1.if`: the reference for `data`, `octet&`
  and prototype interfaces.
