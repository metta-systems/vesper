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

Implemented: the BCM2836 local controller with the cascaded BCM2835 ARM-control block (Raspberry Pi 3). The nucleus claims each pending line, re-arms its timer on the physical-timer line, and masks every other line, since lines cannot be bound yet. See [`irq-controller-placement.md`](../../irq-controller-placement.md) for the design survey.

## Planned changes

- The GICv2 component (Raspberry Pi 4 and 5), using split priority-drop/deactivate so a line stays active until its handler acknowledges it.
- Operations to bind a Notification and acknowledge an interrupt: on delivery the nucleus masks the line (or leaves it active, on the GIC) and signals the Notification; acknowledging unmasks or deactivates it.
