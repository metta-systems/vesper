# IRQHandler

| | |
|---|---|
| Wire type | `0x87` (arch index 7) |
| Pool | none |
| Status | Deferred: dispatch returns `UnsupportedArchType` |

## Purpose

An `IRQHandler` capability is intended to name one bound interrupt line: the
holder arranges delivery of that interrupt (typically as a signal on a
`Notification` object) and acknowledges/controls the line. This is the
per-interrupt half of the interrupt model — the kernel translates hardware
interrupts into invocations of device drivers' handlers, with all policy
(who gets which interrupt, what happens on delivery) delegated to userspace.

## User-level visible operations

None active. The intended vocabulary, following the seL4 model
(`IRQHandler.SetNotification/Ack`), would be:

| Intended operation | Semantics | Status |
|---|---|---|
| Bind/SetNotification | Select the Notification object that the interrupt signals | Deferred |
| Ack/Acknowledge | End-of-interrupt, re-enabling the line | Deferred |
| (possibly) unbind | Release the binding | Deferred |

No operation IDs are assigned; do not silently reuse numbers when the
contract is selected.

## Kernel-level implementation details

- `ArchType::IRQHandler` is defined in the catalogue;
  `ArchObjects::invoke_irq_handler` provides the default
  `UnsupportedArchType` rejection. No object struct, pool, or handler
  exists.
- No interrupt-controller HAL exists in the kernel: the vault's
  `Interrupts.md` research note (RPi2/3 legacy controller, RPi4 GICv2,
  distributor/CPU-interface configuration, priority drop/EOI) is the
  intended basis, but none of it is implemented.
- The natural delivery target is the active `Notification` object (bitmap
  coalescing, one-consumer waiters) — the composition is designed
  (see [notification.md](../core/notification.md)) but no IRQ→Notification
  binding path exists.

## Sidenotes

- Interrupt authority must originate from authorized hardware-resource
  assignment (an `IRQControl`-style issuance), not arbitrary retype —
  consistent with the ASID model of hardware namespaces.
- Timer interrupt support is a prerequisite for the Time subsystem (finite
  timeouts, scheduling) — the IRQ family is therefore on the critical path of
  Phase 7, not merely a device-driver concern.

## TODOs

- Interrupt-controller HAL (RPi3 legacy controller first, GICv2 for RPi4) —
  vault research note is the checklist.
- Handler contract: operation schemas, binding semantics, acknowledgment
  model, level vs edge behavior — D4/D9.
- Interaction with the completion foundation (does an IRQ delivery ever
  interact with pending records, or only with Notification state?) — D7.
- Multiprocessor routing and affinity — unexamined.

## Cross-reference: implementation vs. desired capabilities (🧠 Vesper vault)

- `Vesper.md` (vault): "Interrupts come from hardware, usually in privileged
  mode and kernel is responsible for translating them into invocations of the
  device drivers' handlers. This is a hardware mechanism, all policy
  decisions about interrupts are delegated to the library OS." —
  **unimplemented but direction-consistent**: the selected model (IRQHandler
  + Notification delivery, policy in userspace) matches; nothing exists in
  the kernel yet.
- `Interrupts.md` (vault): detailed GICv2/legacy-controller research
  checklist (distributor config, ICC_IAR acknowledge, priority drop/EOI,
  VBAR_EL0/EL1, timer/serial interrupts) — **all unchecked todos**; no HAL,
  no timer, no serial interrupt exists. This is the largest fully-open
  hardware-facing gap against the vault notes.
- `Vesper Capabilities (from wiki).md` (vault): "System support for
  interrupts includes enabling/disabling appropriate interrupt lines and
  access to capabilities that will be called when interrupts arrive" —
  **matches the intended IRQHandler/IRQControl split**; unimplemented.
- `API/fbufs.md` (vault): `irq_notify` in the NetRxChannel pattern — the
  userspace composition this kind must eventually enable.
