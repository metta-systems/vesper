# Components implementation plan

The checklist for EL0 components, Meddle interfaces and Kickstart's component wiring. The
rules are in [components-design.md](components-design.md#decisions), open questions only in [components-decisions.md](components-decisions.md). The working rules,
validation commands and `just` recipes of the
[capabilities plan](capabilities-implementation-plan.md) apply here unchanged.

Mark an item `- [x]` only after implementation and validation have passed. A finished item
leaves a one-line marker in [components-completed.md](components-completed.md).

- [ ] Grow the `mailbox_v1.property` whitelist as components need tags (clock and power setters for `cprman`, memory tags for `vcsm` once their policy exists).
- [ ] Port the rest of the old `Power::off` into `power`: the GPIO pin power-down before the halt (requires a GPIO component).
- [ ] Asynchronous submit and complete for interfaces with data areas: a request queue in the area and EventCount completion, for functions that should not hold the caller's Thread until the answer (such as `property`).
- [ ] `sdhost-bcm2835`: wait for the controller's interrupt instead of polling, and move several blocks per command (CMD18/CMD25); then bundle it in the production image and validate it on a Raspberry Pi 3.
- [ ] `sdhci`: validate on a Raspberry Pi 4 (EMMC2: ADMA2 into bounce pages below the 1 GiB DMA limit, the base clock from the firmware's EMMC2 clock if the capabilities give none) and a Raspberry Pi 5 (`brcm,bcm2712-sdhci`: the 200 MHz fixed clock, the configuration block's forced card detect and SD pin select, registers above 4 GiB) with `just boot-board-test`: its `sdhci:` lines on the UART name the base clock, ADMA2, the card's size and block 0's master boot record; the boot card stays at 3.3 V and 25 MHz.
- [ ] `pcie-brcmstb`: validate on a Raspberry Pi 4 (link up, the VL805 `1106:3483` behind the root port with its BAR in the 0xc000_0000 window, the software-raised MSI arriving on line 180) and a Raspberry Pi 5 (rescal and bridge resets, the 54 MHz PLL, RP1 `1de4:0001` with its BARs, the MIP's last vector arriving on GIC SPI 191) with `just boot-board-test`, from its `pcie:` lines on the UART.
- [ ] PCI child drivers, first slice (`pci_v1` `address`/`claim`/`raise_msi` in `pcie-brcmstb`, Kickstart's `nodes = [...]` matching and `PciHost::children`, the PCI address cells of a bridge's `dma-ranges` in `dma_address`): validate with `just boot-board-test` on a Raspberry Pi 4 (`xhci:` lines of `usb-hcd-xhci`, the VL805's driver: `1106:3483` claimed with its BAR0, a No Op command completing on its MSI) and a Raspberry Pi 5 (`pci-probe:` lines: RP1 `1de4:0001` claimed with its BARs, `raise_msi` refused with `no_msi`).
- [ ] RP1 interrupts: `pcie-brcmstb` programs RP1's MSI-X vector *n* to MIP vector *n*; Kickstart binds the MIP line of each RP1 child's interrupts to the child's Notification; RP1 children claim their register sub-ranges of BAR1 (placed at the bus address the `rp1` node's `ranges` give). Needs a first RP1 child driver.
- [ ] `pcie-brcmstb`: pass the node's `brcm,vdm-qos-map`/`brcm,fifo-qos-map`, `max-link-speed`, `aspm-no-l0s` and `brcm,clkreq-mode` instead of assuming the boards' values.
- [ ] Move endpoint-test's fixture init pages and hand-wired Invocations onto the init record and the generated-interface import wiring Kickstart gives drivers (`DriverInit`, `imports.if`).
- [ ] Move the Bounce fixture (sync-test, ppc-test, via `libkicktest::bounce`) onto EL0 components where it does not need EL1 register access.
- [ ] A read-only filesystem over `block_v1` (exFAT or ext4), with a file-finding interface, on the way to Mimisbrunnr's own storage interface.
