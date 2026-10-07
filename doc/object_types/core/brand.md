# Brand

| | |
|---|---|
| Wire type | `0x06` (core) |
| Pool | none — no object exists |
| Status | Contract direction recorded; not implemented (dispatch returns `UnsupportedCoreType`) |

## Purpose

A `Brand` capability is intended to name a target for inter-processor
interrupts and Thread-handler upcalls. It is a distinct core kind; what
exactly it names, how its handler is bound, and how it relates to the
architecture `IRQHandler` kind are open.

## Sidenotes

- Brand is deferred to the IRQ/upcall work; it is recorded now so its wire ID
  is reserved for that purpose.

## TODOs

- What a Brand names, and how its target/handler binding is established.
- Its relation to Thread capabilities and `IRQHandler`.
- Invocation and delivery semantics, rights, lifetime and revocation.
- ABI, wrapper, kernel object and architecture integration in the IRQ/upcall
  slice.
