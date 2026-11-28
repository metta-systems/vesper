# Components open decisions

Parking lot for unanswered component decisions. Each entry stays open until the maintainer
answers it explicitly. An answered question becomes a numbered section of
[components-design.md](components-design.md#decisions) and leaves this file; its tasks go into
[components-implementation-plan.md](components-implementation-plan.md).

No component decision is open.

## Future work

Not decisions to make now and not planned work; revisit only if the project needs them.

- **N-COMPONENTS-1: driver probe/init calls.** Kickstart hands a driver everything through its init page and never calls into it. A device manager may need a probe/init part of the driver interface (asking a driver whether it can drive a device, or telling it to start or stop); look into what that would be and how it is called.
- **N-COMPONENTS-2: per-client mailbox policy.** `mailbox_v1.property` applies one fixed tag whitelist to every client. Different rules per client (for example clock and power setters for `cprman` only, memory tags for `vcsm`) would key the policy on the caller's badge (`DriverInit::area_for_badge`).
- **N-COMPONENTS-3: resources named in `uses`.** declare the boot-image resources a component
  maps in its `imports.if` as `uses resource font;`, in place of the
  `resources = [...]` list of its manifest entry. Meddle's `uses` parser, AST
  and summary would carry the resource names (their order stays the index in
  `DriverInit::resources`), and image-build would read them from the imports
  file. Today the manifest list does the job.
- **N-COMPONENTS-4: universal channel provisioning.** a general way to set up shared control
  pages and data areas between components, instead of per-interface wiring (the
  per-pair data areas Kickstart builds for Meddle `data` declarations, the
  interrupt-IN channel of the USB stack and the hand-wired serial channel are
  three cases of it). Would need its own design, for example a Meddle `channel`
  declaration shared by all of them, or client-lent Frames with a mapping window.
- **N-COMPONENTS-5: Arasan SDHCI for the card on the boards.** the Raspberry Pi 3 has two SD
  controllers, and the device tree says which one has the card (the enabled
  node whose pin group holds GPIO 48 to 53). The stock tree enables SDHOST and
  gives the Arasan to the Wi-Fi chip, so the `sdhci` driver serves
  `brcm,bcm2835-sdhci` only in `qemu` builds; a tree that enables the Arasan
  for the card instead (the `sdio` overlay) would need the driver to tell the
  card's node from the Wi-Fi chip's, for example by that pin group.
- **N-COMPONENTS-6: zero-copy DMA for packet drivers.** a driver that moves a client's
  buffers by DMA (the `sdhci` driver's ADMA2 today, xHCI and network drivers
  later) does not know the physical addresses of the channel data pages its
  clients share with it, so the controller moves data through bounce pages of
  the driver's own memory and the CPU copies every byte once more between
  those and the client's buffers (as the DWC2 driver does). DMA then saves the
  wait on the controller, not the copy. Avoiding the copy needs packet-driver
  infrastructure that tells a driver the bus addresses of the pages it is
  given: Kickstart recording them for the channel pages it maps, client-lent
  Frames whose extent the driver reads (`Frame.GetAddress`), or a channel
  whose data pages the driver allocates from its own memory. It also needs the
  pages in the device's DMA window (the BCM2711's first gigabyte for EMMC2)
  and a rule for cache maintenance on pages two components map. Ties in with
  N-COMPONENTS-4.
- **N-COMPONENTS-7: the activity LED handed to its driver.** `mailbox_v1.set_led` picks the
  LED's firmware GPIO from the board revision code the firmware reports
  (`mailbox_impl::activity_led_pin`), so the mailbox component knows boards.
  Kickstart could instead read the device tree's `/leds` node
  (`libdevicetree`'s `DeviceTree::activity_led`) and hand the LED to whoever
  switches it through the driver init page: a firmware GPIO to the mailbox
  component, a SoC GPIO to the GPIO component, the Raspberry Pi 5's
  `brcm,brcmstb-gpio` pin to a driver for that block.
