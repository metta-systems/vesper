# ASIDPool

| | |
|---|---|
| Wire type | `0x83` (arch index 3) |
| Backing | Kernel ASID pool, created at boot |
| Operations | Assign |

## Purpose

An ASIDPool hands out ASIDs (address-space identifiers) from a range of 512. The hardware tags cached translations with the ASID of their AddressSpace, so an AddressSpace needs an ASID before it can be activated, and switching between AddressSpaces needs no TLB flush. The first component receives the boot pool at `KeySlot::BOOT_ASID_POOL` (slot 14). Retype cannot create ASID pools.

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Assign | `x2` target AddressSpace | `Grant` on the pool, `Map` on the AddressSpace | `x1` the assigned ASID |

### Assign

Allocates a free ASID from the pool and binds it to the AddressSpace named by `x2`. The AddressSpace must already have a root ([`PageTable.Map`](page_table.md#map)) and no ASID. The ASID stays bound until [`AddressSpace.Retire`](address_space.md#retire) returns it to the pool.

| Error | Cause |
|---|---|
| `InsufficientRights` | The pool lacks `Grant`, or the AddressSpace lacks `Map` |
| `NotMapped` | The AddressSpace has no root |
| `AlreadyMapped` | The AddressSpace already has an ASID |
| `AsidPoolExhausted` | Every ASID in the pool is in use |
| `TypeMismatch` | `x2` is not an AddressSpace |
| `InvalidOperation` | A nonzero unused argument |

A failed Assign leaves the pool and the AddressSpace unchanged.

## Implementation

The boot pool covers ASIDs 0–511 and never hands out ASID 0, which the kernel uses for its own boot context.
