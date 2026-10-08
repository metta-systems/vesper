# IRQHandler

| | |
|---|---|
| Wire type | `0x87` (arch index 7) |
| Backing | None |
| Status | Not implemented |

## Purpose

An IRQHandler will represent one interrupt line. Its holder will bind the line to a [Notification](../core/notification.md), so that each interrupt signals the Notification, and acknowledge the interrupt when handled. Invoking an IRQHandler capability fails with `UNSUPPORTED_ARCH_TYPE`.

## Planned changes

Interrupt-controller support for the Raspberry Pi 3 interrupt controller and the GICv2 on the Raspberry Pi 4, with operations to bind a Notification and acknowledge an interrupt.
