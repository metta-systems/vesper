# IRQHandler

| | |
|---|---|
| Wire type | `0x0a` (core) |
| Backing | A record in the kernel's interrupt-line table |
| Operations | SetNotification, Clear, Ack, Retire |

## Purpose

An IRQHandler names one interrupt line, issued by [IRQControl](irq_control.md#issue). Its holder binds the line to a [Notification](notification.md), so that each interrupt signals that Notification, and re-enables the line after handling each interrupt. A driver waits for its interrupts with a receiver Thread that has its own [Time](time.md).

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | SetNotification | `x2` Notification, `x3` receiver Thread | `Write`; `Send` on the Notification; `Write` on the Thread | — |
| 1 | Clear | — | `Write` | — |
| 2 | Ack | — | `Write` | — |
| 3 | Retire | — | `Retire` | — |

Unused argument registers must be zero (`InvalidOperation` otherwise). `IrqHandlerKey` in `libs/object` wraps every operation.

### SetNotification

Binds the line to the Notification `x2` for the receiver Thread `x3`. A line that is already bound fails with `AlreadyMapped`; call [Clear](#clear) first to rebind it. Each interrupt signals the Notification with the badge of the `x2` capability, which must be nonzero. The receiver must have its own Time, bound by [`Scheduler.ConfigureThread`](scheduler.md#configurethread).

| Error | Cause |
|---|---|
| `InvalidOperation` | The Notification capability has badge zero; the receiver has no Time |
| `InsufficientRights` | A missing right |
| `TypeMismatch` | A key names the wrong kind |
| `InconsistentKey` | The IRQHandler, Notification or Thread is retired |
| `AlreadyMapped` | The line is already bound |

### Clear

Removes the binding. Later interrupts on the line are masked and dropped.

### Ack

Enables the line. A freshly issued line is masked, and the kernel masks the line again on every delivery, so a driver acks once after binding and once after handling each interrupt:

1. `IRQControl.Issue`, then `SetNotification`.
2. `Ack`.
3. The receiver waits on the Notification, quiets the device, then acks again.

### Retire

Clears the binding, masks the line and frees it for a new `IRQControl.Issue`. Every copy of the IRQHandler key then fails with `InconsistentKey` (`ObjectRetired`).

## Interrupt controllers

The interrupt controller is driven by a **privileged component**: a position-independent EL1 image bundled in the boot image (`drivers/irqchip-*`, interface in `libirqchip`). Kickstart picks the component whose `compatible` list names the device tree's root interrupt controller (the CPU timer's `interrupt-parent`), relocates it into the kernel half, maps the controllers' MMIO after it, and hands the kernel its operation table (`init`, `claim`, `complete`, `mask`, `unmask`, `set_trigger`, `xlate`). One image carries every component; the choice is made at boot. Lines are component-defined numbers; device tree interrupt specifiers become lines, with the trigger they ask for, through `xlate`.

| Component | Controller | Boards | Lines |
|---|---|---|---|
| `drivers/irqchip-bcm2836` | BCM2836 local controller with the cascaded BCM2835 ARM-control block | Raspberry Pi 3 | Local hwirq, then `32 + 32 * bank + irq` |
| `drivers/irqchip-gicv2` (via the `arm-gic` crate) | GIC-400 | Raspberry Pi 4 and 5 | INTIDs |

The GICv2 component routes every shared peripheral interrupt (SPI) to the booting core when it binds. It uses only the Non-secure view: it acknowledges and ends interrupts through the banked `GICC_IAR`/`GICC_EOIR`, which serve Group 1 there, the group the firmware assigns. Running Non-secure on real hardware is not verified yet; QEMU's raspi4b GIC has no Security Extensions.

## Implementation

Kickstart carves the line table from the boot Untyped, one record per line of the component. On an interrupt the kernel claims the pending line; the timer line re-arms the kernel's tick. Any other line is masked; a bound line also signals its Notification, waking the front waiter, and the kernel then ends the interrupt (on the GIC, priority drop and deactivation together). A line whose Notification was retired is treated as unbound. A woken receiver joins the kernel's first-in, first-out run queue; it runs at the next tick, or at once when the CPU was idle.

## Planned changes

Direct switch to the interrupt handler's thread if it's priority is higher than the currently executing thread. This may cause hardware to starve the running app so needs to be investigated first.
