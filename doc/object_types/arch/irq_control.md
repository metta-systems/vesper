# IRQControl

| | |
|---|---|
| Wire type | `0x88` (arch index 8) |
| Pool | none |
| Status | Deferred: dispatch returns `UnsupportedArchType` |

## Purpose

An `IRQControl` capability is intended as the *issuance* authority for
interrupt lines: the root, singleton-style capability (one per interrupt
controller) through which `IRQHandler` capabilities for individual interrupts
are minted. It separates "who may allocate interrupt lines at all" from "who
holds a specific line" — the same hardware-namespace pattern as ASID pools:
interrupt authority originates from authorized hardware-resource assignment,
never from arbitrary retype.

## User-level visible operations

None active. The intended vocabulary, following the seL4 model
(`IRQControl.Get`), would be:

| Intended operation | Semantics | Status |
|---|---|---|
| Get/Issue | Mint an `IRQHandler` capability for one interrupt number into a destination slot | Deferred |

No operation IDs are assigned; do not silently reuse numbers when the
contract is selected.

## Kernel-level implementation details

- `ArchType::IRQControl` is defined in the catalogue;
  `ArchObjects::invoke_irq_control` provides the default
  `UnsupportedArchType` rejection. No object struct, pool, or handler exists.
- As with [IRQHandler](irq_handler.md), no interrupt-controller HAL exists;
  the vault's `Interrupts.md` research note is the intended implementation
  basis.
- The issuance flow would presumably mirror the boot-grant model: Kickstart
  holds the control capability (or grants it to a userspace interrupt
  manager), which then hands out per-line handlers — but no bootstrap layout
  is decided (D4).

## Sidenotes

- The control/handler split keeps a single privileged issuance point per
  hardware namespace while allowing per-line authority to be delegated
  freely — consistent with the capability model's "no ambient authority"
  tenet.
- Whether IRQControl is one capability for all lines or per-controller
  (GIC distributor vs CPU interface, legacy vs GICv2) is an open shape
  question tied to the HAL design.

## TODOs

- Everything: issuance schema (which argument names the interrupt number,
  where the handler lands), multiplicity (per-controller vs singleton),
  revocation of issued handlers, and interaction with the KeyTable
  derivation allowlist (handlers are not on the CopyDerive allowlist today)
  — D4/D9, blocked on the interrupt-controller HAL.

## Cross-reference: implementation vs. desired capabilities (🧠 Vesper vault)

- `Vesper Capabilities (from wiki).md` (vault): "System support for
  interrupts includes **enabling/disabling appropriate interrupt lines** and
  access to capabilities that will be called when interrupts arrive" — the
  enable/disable half maps to this control kind; **unimplemented**.
- `seL4 Capabilities.md` / `API/seL4 API.md` (vault): seL4's
  `seL4_IRQControl.Get` issuance model — the intended template; no contract
  selected in Vesper yet.
- `Interrupts.md` (vault): the HAL research checklist underpins both IRQ
  kinds; entirely unimplemented (see [irq_handler.md](irq_handler.md) for the
  itemized gap).
- `Prototype.md` (vault): `cap_irq_control_cap = 14`,
  `cap_irq_handler_cap = 30` — numbering superseded by the canonical arch
  baseline (`0x88` / `0x87`).
