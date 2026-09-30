# Brand

| | |
|---|---|
| Wire type | `0x06` (core) |
| Status | Contract direction recorded; implementation deferred until IRQ work |

## Purpose

A `Brand` capability is intended to identify a target for IPI and thread-handler
upcalls. It is a distinct capability kind in the core catalogue.

The precise target identity, handler binding, operation set, authority rules,
and relationship to the architecture-specific `IRQHandler` capability remain
open. Brand is recorded as a design direction, not an implemented IRQ or
upcall mechanism.

## User-level visible operations

No operation schema is selected. Brand implementation is deferred until the
IRQ/upcall work is designed.

## TODOs

- Define what a Brand names and how its target/handler binding is established.
- Define its relation to Thread capabilities and `IRQHandler`.
- Specify invocation and delivery semantics, rights, lifetime, and revocation.
- Implement and validate the shared ABI, userspace wrapper, kernel API/object,
  and architecture integration in the IRQ/upcall slice.
