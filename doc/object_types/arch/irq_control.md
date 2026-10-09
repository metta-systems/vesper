# IRQControl

| | |
|---|---|
| Wire type | `0x88` (arch index 8) |
| Backing | None |
| Status | Not implemented |

## Purpose

IRQControl will issue [IRQHandler](irq_handler.md) capabilities: its holder decides which component receives which interrupt line. Invoking an IRQControl capability fails with `UNSUPPORTED_ARCH_TYPE`.

## Planned changes

IRQControl is a boot key installed for kickstart. Kickstart is only the init component: it spawns an EL0 `irq-manager` component and moves IRQControl into it for good. `irq-manager` owns line policy — which driver gets which line, trigger configuration — and hands each driver component the IRQHandler capabilities for its device through a device-scoped PPC key.
