# IRQ open decisions

Parking lot for unanswered interrupt-delivery decisions. Each entry stays open until the
maintainer answers it explicitly. An answered question becomes a numbered section of
[irq-design.md](irq-design.md#decisions) and leaves this file; its tasks go into
[irq-implementation-plan.md](irq-implementation-plan.md).

## Q-IRQ-1: one Notification for server and interrupt waits

Whether a Thread may bind one Notification for both PPC-server and interrupt waits
(seL4 `TCB_BindNotification`).
