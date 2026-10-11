# IRQControl

| | |
|---|---|
| Wire type | `0x09` (core) |
| Backing | None: the authority over every interrupt line |
| Status | Issue |

## Purpose

IRQControl issues [IRQHandler](irq_handler.md) capabilities: its holder decides which component receives which interrupt line. Interrupt authority comes only from IRQControl; no Untyped can be retyped into it. The first component receives it at `KeySlot::IRQ_CONTROL` (19).

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Issue | `x2` destination table, `x3` destination slot, `x4` cell count and controller, `x5..x7` specifier cells | `GRANT` on the IRQControl, `INSTALL` on the table | `x1` IRQHandler key, `x2` line |

`IrqControlKey::issue` in `libs/object` wraps the operation.

### Issue

Decodes a device tree interrupt specifier into a line of the interrupt controller, configures the line's trigger from the specifier, and installs an IRQHandler for the line at slot `x3` of the table `x2`. The new IRQHandler has all rights.

| Register | Contents |
|---|---|
| `x4` bits 7:0 | Number of specifier cells, 1 to 3 |
| `x4` bits 15:8 | Index of the controller the specifier is relative to (see below) |
| `x4` bits 63:16 | Zero |
| `x5..x7` | The cells, in device tree order; cells beyond the count are zero |

The controller index follows the interrupt controller component's list: the root controller (the CPU timer's `interrupt-parent`) is 0, and the cascaded controllers it drives follow in device tree order. Kickstart records their phandles in this order in `BOOT_INFO` (`kickstart::bootstrap::interrupt_controllers`), so a node's `interrupt-parent` phandle gives its index.

The line stays masked until the IRQHandler's first [Ack](irq_handler.md#ack).

| Error | Cause |
|---|---|
| `IRQ_LINE_ACTIVE` | The line already has a live IRQHandler, or it is the kernel's timer line; detail 1 is the line |
| `INVALID_OPERATION` | A cell count outside 1–3, an unknown controller, nonzero reserved bits or unused cells, a cell wider than 32 bits, a specifier the controller cannot decode, or a trigger the line cannot take |
| `SLOT_OCCUPIED` | The destination slot holds a key |
| `INSUFFICIENT_RIGHTS` | `GRANT` or `INSTALL` is missing |
| `TYPE_MISMATCH` | `x2` is not a KeyTable |

## Implementation

Lines are the interrupt controller component's own numbers (see [interrupt controllers](irq_handler.md#interrupt-controllers)). A specifier that sets a trigger (the GIC's flags cell) configures the line as edge or level triggered; the BCM2836 lines are level triggered and take no trigger.

## Planned changes

Kickstart will hand IRQControl to an EL0 `irq-manager` component, which gives each driver the IRQHandlers for its device.
