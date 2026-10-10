# IRQHandler

| | |
|---|---|
| Wire type | `0x0a` (core) |
| Backing | None |
| Status | Not implemented |

## Purpose

An IRQHandler names one interrupt line. Its holder binds the line to a [Notification](notification.md), so that each interrupt signals that Notification, and re-enables the line after handling it. A driver waits for its interrupts with a receiver Thread that has its own [Time](time.md). Invoking an IRQHandler capability fails with `UNSUPPORTED_CORE_TYPE`.

## Interrupt controllers

The interrupt controller is driven by a **privileged component**: a position-independent EL1 image bundled in the boot image (`drivers/irqchip-*`, interface in `libirqchip`). Kickstart picks the component whose `compatible` list names the device tree's root interrupt controller (the CPU timer's `interrupt-parent`), relocates it into the kernel half, maps the controllers' MMIO after it, and hands the nucleus its operation table (`init`, `claim`, `complete`, `mask`, `unmask`, `set_trigger`, `xlate`). One image carries every component; the choice is made at boot. Lines are component-defined numbers; device tree interrupt specifiers become lines through `xlate`.

| Component | Controller | Boards | Lines |
|---|---|---|---|
| `drivers/irqchip-bcm2836` | BCM2836 local controller with the cascaded BCM2835 ARM-control block | Raspberry Pi 3 | Local hwirq, then `32 + 32 * bank + irq` |
| `drivers/irqchip-gicv2` (on the `arm-gic` crate) | GIC-400 | Raspberry Pi 4 and 5 | INTIDs |

The GICv2 component uses only the Non-secure view: it acknowledges and ends interrupts through the banked `GICC_IAR`/`GICC_EOIR`, which serve Group 1 there, the group the firmware assigns. Running Non-secure on real hardware is not verified yet; QEMU's raspi4b GIC has no Security Extensions.

## Implementation

The nucleus claims each pending line and re-arms its timer on the physical-timer line. Every other line is masked and ended, because no line can be bound yet.

## Planned changes

Operations SetNotification (0), Clear (1) and Ack (2). On delivery the nucleus will signal the bound Notification with its badge, mask the line and end the interrupt at the controller (on the GIC, priority drop and deactivation together); Ack unmasks the line.
