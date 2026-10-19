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
  channel with its events unmasked, then sleeps on `completions` with the remaining
  deadline (500 ms), so a missing interrupt ends it with `transfer_failed`. Port
  events stay masked. `GINTSTS.CurMod` is 1 in host mode, and the component waits
  for that bit.
- Kickstart caps a node at `MAX_DEVICE_PAGES` (32). The serial channel's control
  page sits at `0x3802_0000`, above the register pages.
- `drivers/usb-hcd-dwc2` resets the root port and serves `usb_hcd_v1` for control
  pipes (generated crates `drivers/usb-hcd-dwc2/server` and `client`). It does not
  enumerate: a client resets the port, opens a pipe to address 0 and enumerates
  through `vesper_usb_protocol`. `usb-test-client` does so in `just test-usb` and
  finds the hub (class 9) at address 1.
- A call runs on the caller's Thread, one at a time (a `Turnstile`). The client
  stages the setup packet and data stage in its data area; the component copies
  them to and from its DMA page (data stage at most 4032 bytes). `DataArea` has
  byte access for this (`libs/meddle-runtime`).
- Pipes are handles in a 16-entry table: index plus generation, never zero. The handle
  is the badge of the pipe's `pipe_v1` keys, so a closed pipe's keys fail even when its
  slot is reused. The caller's data area (found from the stack the call runs on)
  still bounds the stage a control call may name. A pipe is opened to an endpoint
  address (bit 7 is the direction) and configured with `pipe_v1.configure` (kind,
  `max_packet`, and the polling interval of an interrupt pipe), again whenever a
  device's real control packet size is learned. A control pipe's `max_packet` is
  8, 16, 32 or 64; interrupt and bulk pipes raise `not_permitted`.
- `set_route` stores the route, and `control` on a routed pipe runs each packet as a
  split transaction: a start split, then complete splits until the hub has the
  result (`split::Sequence`). A high-speed pipe cannot be routed.
- `drivers/usb/protocol` has the setup packet, descriptor walker, enumeration state
  machine and HID boot-report decoder. The state machine has no retry and no
  timeout; the caller supplies delays and port resets as steps. Keyboard decoding
  ignores roll-over reports. Not covered: hub descriptors, class requests beyond
  SET_CONFIGURATION, HID report descriptors (boot protocol only).

## Layers

Modelled on CSUD (<https://github.com/Chadderz121/csud>), split into components
that Kickstart wires statically:

```
usb_hcd_v1 (dwc2 HCD)  <- control/pipe calls ->  usb_v1 (usbd: core + hub)  <- claim/control ->  hid
        ^                                                                                       |
        +------------- interrupt-IN reports over io-channel, bypassing usbd ----------------------+
                                                                              hid -> input_v1 events (io-channel) -> shell
```

| Component | Directory | Exports | Imports |
|---|---|---|---|
| `dwc2` (HCD) | `drivers/usb-hcd-dwc2/` | `usb_hcd_v1` | none |
| `usbd` (core, hub inside) | `drivers/usb/` | `usb_v1` | `usb_hcd_v1` |
| `hid` (keyboard, mouse) | `drivers/usb-hid/` | `input_v1` | `usb_v1`, `usb_hcd_v1` |

- The HCD does registers, DMA and the schedule. It owns the MMIO frame, the
  IRQ Notification and a DMA Untyped, and runs two threads as the PL011 driver
  does.
- `usbd` runs enumeration, descriptors and hub polling. It is control-plane
  only.
- `hid` claims boot-protocol keyboards and mice, posts empty IO_Recs for
  interrupt-IN, and decodes boot reports with a pure library. It imports
  `usb_hcd_v1` as well, because its interrupt pipe is an HCD handle.

## Control and data planes

- Control plane (Invocations): enumeration, control transfers, SET_PROTOCOL,
  SET_IDLE, LED output reports. Rare and RPC-like.
- Data plane (io-channel): interrupt-IN HID reports. Client posts empty IO_Recs;
  the HCD fills them on completion and advances the client's EventCount. With no
  empty IO_Rec posted, the HCD does not schedule the IN transfer, so backpressure
  is natural. The drop-and-count rule of `io-channel-design` remains a safety net.
- Data areas: control transfers stage the setup packet and data stage in the
  client's `data` area (`octet&`), following the mailbox convention.

## Interfaces (drafted)

- **Hub location (maintainer decision, 2026-10-18).** The hub driver is inside `usbd`,
  not a separate component. Its pure logic (hub descriptor, class requests, port
  status, per-port state machine) goes in a `hub` module of `vesper-usb-protocol`,
  so it stays host-testable and could be extracted later.
- **Pipe authority (maintainer decisions, 2026-10-18 and 2026-10-19).** A pipe is a set of
  Invocation keys issued by the host controller, not a number checked against caller
  identity. `pipe_v1` is a `local` interface of its own, one key per function, all
  badged with the pipe's handle; `usb_hcd_v1.open_pipe` installs the keys in a key table
  the caller lends and returns their values in the data area. Holding a key is the right
  to use the pipe, so `usbd` can hand a pipe to `hid` by installing a narrowed copy in
  `hid`'s table. The mechanism is in the
  [contract](capabilities-contract.md#component-interfaces-and-imports).
- `drivers/usb-hcd-dwc2/interfaces/usb_hcd_v1.if`: `reset_port`, `port_status`,
  `open_pipe`. Parameters are DWC2-neutral. `open_pipe` takes the first slot of the
  lent table, the device address, the endpoint address and the speed, and installs
  the keys of `pipe_v1`.
- `drivers/usb-hcd-dwc2/interfaces/pipe_v1.if`: `configure`, `set_route`, `close_pipe`, `clear_halt`,
  `control`, the functions of one open pipe; the split-transaction route is set
  separately with `set_route`.
- `drivers/usb/interfaces/usb_v1.if`: `claim_interface` (blocks until a matching
  device appears), `release`, `interrupt_pipe`, `control`.
- `drivers/usb-hid/interfaces/input_v1.if`: `set_leds` today, to be replaced by `descriptor` and `snapshot` (USB4). Events travel over an
  io-channel whose records are defined in [input-design.md](input-design.md).
- `imports.if` files: `usbd` uses `usb_hcd_v1` via `compatible "brcm,bcm2708-usb"`
  (the DWC2 node's compatible string, to be verified). `usb-hid` is comment-only
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
- RPi4 uses a VL805 xHCI on PCIe. See the RPi4 section below.

## RPi4 follow-up (after RPi3 works)

Not in scope for the RPi3 work. Recorded here so the HCD interface stays
compatible. Sources are mostly 2020-era patches, so check current Raspberry Pi
kernel and firmware sources before implementing.

Hardware: USB-A ports sit on a VL805 xHCI behind the BCM2711 PCIe controller
(`pcie0`, `brcm,bcm2711-pcie` at 0x7d500000). The Pi 4 DMA window is capped at
the first 3 GB of RAM, so xHCI buffers must come from below that.

Bring-up, in order:

1. **PCIe.** Bring the link up, set the CPU-to-PCI and PCI-to-memory windows,
   find the VL805 and program its BAR so the xHCI registers are mapped. (From
   memory; the search did not give the exact sequence.)
2. **VL805 firmware.** After a PCIe reset the VL805 loads firmware from an EEPROM
   if one is present, otherwise from VideoCore. Pi 4B 1.4 dropped the SPI EEPROM,
   so VideoCore must load it there. It is a firmware mailbox request sent after
   PCIe configuration and before xHCI starts
   ([U-Boot patch](https://lists.denx.de/pipermail/u-boot/2020-April/409300.html)).
   Needs a new `mailbox_v1` property tag. Tag ID and current firmware behaviour
   unverified. Without the load the xHCI can hang.
3. **xHCI core.** Wait for CNR to clear in USBSTS; set MaxSlotsEn in CONFIG;
   point DCBAAP at the device context base array (plus scratchpad buffers if
   HCSPARAMS2 asks for them); set up the command ring and event ring
   (ERSTBA, ERSTSZ, ERDP); set Run/Stop. Smoke test with a no-op command and its
   completion event
   ([Agilex xHCI programming model](https://intel.com/content/www/us/en/docs/programmable/814346/24-3/programming-model-56434.html),
   [Linux xhci.h history](https://git.linaro.org/plugins/gitiles/kernel/linux-linaro-stable.git/+/33b2831ac870d50cc8e01c317b07fb1e69c13fe1%5E%21/drivers/usb)).
4. **Interrupts.** The VL805 normally signals MSI, which the BCM2711 PCIe block
   should route to the GIC. Unverified.
5. **Enumeration.** Per root port: reset it (USB 2 ports need this before they are
   enabled), then Enable Slot, Address Device, GET_DESCRIPTOR, Configure Endpoint.
   HID interrupt-IN then runs on the normal ring path. From memory, the VL805 has
   a built-in 4-port hub, so a single keyboard needs no hub driver.

Minimum for a keyboard on RPi4: 1–5 above with control transfers and interrupt-IN.
Hubs, isochronous transfers and xHCI error recovery can wait.

Design impact: an xHCI component implements `usb_hcd_v1` unchanged, so `usbd` and
`hid` carry over. New work is the HCD, PCIe bring-up and the firmware call. The
DMA strategy (answered in USB2, see the status section) also applies to xHCI rings.

## Prior art and candidate crates

Survey from web search on 2026-10-17, not a commitment. Nothing has been checked
for licence, maintenance activity or no_std build status beyond the listing.

- Host side, no_std Rust:
  - [crab-usb](https://docs.rs/crate/crab-usb/0.9.9): async host driver, xHCI only, per search results.
  - [cotton-usb-host](https://docs.rs/cotton-usb-host): no_std, no-alloc host stack, targets the RP2040 controller, not DWC2.
  - [usb-hal](https://lib.rs/crates/usb-hal): early; its host part is listed as unimplemented.
  - [embassy-usb](https://docs.rs/crate/embassy-usb/latest) lists `embassy-usb-synopsys-otg` for the DWC2 family. Whether it supports host mode was not confirmed.
- Device side, not useful for a host HID path:
  - [usb-device](https://github.com/rust-embedded-community/usb-device) and [usbd-hid](https://docs.rs/usbd-hid) (latest 0.7.0 listed as Nov 2023).
  - [usbd-human-device-interface](https://docs.rs/usbd-human-device-interface) is an alternative HID class crate.

No candidate found covers DWC2 host mode and the HID host class in no_std Rust.

## Related

- `doc/agentic/capabilities-contract.md`: PPC, xbufs and EventCounts.
- `drivers/mailbox/interfaces/mailbox_v1.if`: the reference for `data`, `octet&`
  and prototype interfaces.
