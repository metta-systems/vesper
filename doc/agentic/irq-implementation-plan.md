# IRQ implementation plan

The checklist for interrupt delivery and driver loading. The rules are in
[irq-design.md](irq-design.md#decisions), open questions only in
[irq-decisions.md](irq-decisions.md). The working rules, validation commands and `just`
recipes of the [capabilities plan](capabilities-implementation-plan.md) apply here unchanged.

Mark an item `- [x]` only after implementation and validation have passed. A finished item
leaves a one-line marker in [irq-completed.md](irq-completed.md).

- [ ] Hand the boot IRQControl to an EL0 `irq-manager` component for good.
