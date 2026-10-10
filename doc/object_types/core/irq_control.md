# IRQControl

| | |
|---|---|
| Wire type | `0x09` (core) |
| Backing | None |
| Status | Not implemented |

## Purpose

IRQControl issues [IRQHandler](irq_handler.md) capabilities: its holder decides which component receives which interrupt line. Invoking an IRQControl capability fails with `UNSUPPORTED_CORE_TYPE`.

## Planned changes

Operation Issue (0): translate a device tree interrupt specifier into a line, configure its trigger, and install an IRQHandler for it in a given slot. A line has at most one IRQHandler, and the nucleus timer line cannot be issued. IRQControl will be a boot key of Kickstart, which hands it to an EL0 `irq-manager` component; `irq-manager` gives each driver the IRQHandlers for its device.
