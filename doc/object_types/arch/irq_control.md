# IRQControl

| | |
|---|---|
| Wire type | `0x88` (arch index 8) |
| Backing | None |
| Status | Not implemented |

## Purpose

IRQControl will issue [IRQHandler](irq_handler.md) capabilities: its holder decides which component receives which interrupt line. Invoking an IRQControl capability fails with `UNSUPPORTED_ARCH_TYPE`.
