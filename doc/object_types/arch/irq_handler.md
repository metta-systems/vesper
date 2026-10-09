# IRQHandler

| | |
|---|---|
| Wire type | `0x87` (arch index 7) |
| Backing | None |
| Status | Not implemented |

## Purpose

An IRQHandler will represent one interrupt line. Its holder will bind the line to a [Notification](../core/notification.md), so that each interrupt signals the Notification, and acknowledge the interrupt when handled. Invoking an IRQHandler capability fails with `UNSUPPORTED_ARCH_TYPE`.

## Interrupt controllers

The interrupt controller is driven by a **privileged component**: a position-independent EL1 blob bundled in the boot image (`drivers/irqchip-*`, ABI in `libirqchip`). Kickstart picks the component whose `compatible` list names the device tree's root interrupt controller — the CPU timer's `interrupt-parent` — relocates it into the kernel half, maps the controllers' MMIO after it, and hands the nucleus its operation table (`init`, `claim`, `complete`, `mask`, `unmask`, `set_trigger`, `xlate`). One image carries every component; the choice is made at boot. Lines are component-defined numbers; device tree interrupt specifiers become lines through `xlate`.

Implemented:

- **BCM2836** (`drivers/irqchip-bcm2836`): the local controller with the cascaded BCM2835 ARM-control block (Raspberry Pi 3). Lines are the local hwirq, then `32 + 32 * bank + irq` for the cascaded block.
- **GICv2** (`drivers/irqchip-gicv2`, on the `arm-gic` crate): the GIC-400 of the Raspberry Pi 4 and 5. Lines are INTIDs. Only the Non-secure view is used: interrupts are acknowledged and ended through the banked `GICC_IAR`/`GICC_EOIR`, which serve Group 1 there — the group the firmware assigns. Running Non-secure on real hardware is not verified yet; QEMU's raspi4b GIC has no Security Extensions.

The nucleus claims each pending line, re-arms its timer on the physical-timer line, and masks every other line, since lines cannot be bound yet. See [`irq-controller-placement.md`](../../irq-controller-placement.md) for the design survey.

## Planned changes

Operations to bind a Notification and acknowledge an interrupt. Both controllers use the same delivery protocol: on delivery the nucleus masks the line, signals end-of-interrupt (on the GIC, priority drop and deactivation together) and signals the Notification; acknowledging unmasks the line.
