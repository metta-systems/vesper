# IRQ open decisions

Parking lot for unanswered interrupt-delivery decisions. Each entry stays open until the
maintainer answers it explicitly. Answered decisions go into
[irq-design.md](irq-design.md#decisions); their tasks into
[irq-implementation-plan.md](irq-implementation-plan.md).

## IRQ1. One Notification for server and interrupt waits

Whether a Thread may bind one Notification for both PPC-server and interrupt waits
(seL4 `TCB_BindNotification`).
