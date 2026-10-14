# AddressSpace

| | |
|---|---|
| Wire type | `0x82` (arch index 2) |
| Backing | Kernel record carved from the boot Untyped by the boot code |
| Operations | Activate, Retire, CreateInvocation |

## Purpose

An AddressSpace is a protection domain: a translation root, the ASID bound to it, and one KeyTable shared by every Thread running in it. Threads in different AddressSpaces cannot see each other's memory unless both map the same Frame. A Thread runs in one AddressSpace at a time and moves between them with [Invocation](../core/invocation.md) calls.

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Activate | — | `Map` | — |
| 1 | Retire | — | `Retire` | — |
| 3 | CreateInvocation | `x2` entry, `x3` destination table, `x4` destination slot, `x5` stack base, `x6` stack end, `x7` minimum headroom, `x8` key-placement window | `Grant` on the AddressSpace, `Install` on the destination | `x1` new Invocation key |

### Activate

Installs this AddressSpace's translation root and ASID into the hardware (`TTBR0_EL1`). Only the caller's own AddressSpace can be activated. It needs a root ([`PageTable.Map`](page_table.md#map)) and an ASID ([`ASIDPool.Assign`](asid_pool.md)) first. Activating an already active AddressSpace has no further effect.

| Error | Cause |
|---|---|
| `InsufficientRights` | The capability lacks `Map` |
| `NotMapped` | No root or no ASID |
| `InvalidOperation` | Not the caller's own AddressSpace, or the root or ASID does not fit the hardware configuration |
| `InvalidPointer` | The root address cannot be encoded |
| `InconsistentKey` | The AddressSpace was retired (`ObjectRetired`) |

The supported AArch64 configuration is a four-level, 48-bit translation with a 4 KiB granule. ASIDs are 1–255, or 1–65535 where the hardware supports 16-bit ASIDs.

### Retire

Destroys an AddressSpace other than the caller's. Its root must be unmapped first ([`PageTable.Unmap`](page_table.md#unmap) on the root). Retire flushes every TLB entry tagged with its ASID, returns the ASID to its pool and retires the AddressSpace: every capability to it then fails with `ObjectRetired`, and its memory is not reused. Threads still in it fail on their next invocation; retire them with [`Thread.Retire`](../core/thread.md#retire).

| Error | Cause |
|---|---|
| `InsufficientRights` | The capability lacks `Retire` |
| `InvalidOperation` | It is the caller's own AddressSpace, or its root is still installed |
| `InconsistentKey` | The AddressSpace was already retired (`ObjectRetired`) |

### CreateInvocation

Creates an [Invocation](../core/invocation.md) that calls `x2` in this AddressSpace on the stack extent `[x5, x6)` with minimum headroom `x7` bytes, and installs it into slot `x4` of the table named by `x3`. The new capability carries only `Call` right. The entry address must be nonzero (`InvalidPointer`); the stack rules and errors are on the [Invocation](../core/invocation.md#stack-extent) page.

`x8` is the key-placement window that [`CallWithKey`](../core/invocation.md#callwithkey) uses: bits 31:0 are the first slot and bits 63:32 the number of slots, in this AddressSpace's own KeyTable. Zero means the Invocation accepts no lent key. A window that does not fit the table fails with `InvalidSlot`, naming its first slot. The slots need not be vacant when the Invocation is created.

Failures leave the destination table unchanged.

## Implementation

AddressSpaces are created by the boot code, which carves them from the boot Untyped; `Untyped.Retype` cannot create them. When an AddressSpace is created, its KeyTable gets the [return key](../core/thread.md#return) in slot 1. Its translation-table leaves are tagged with its ASID, so switching AddressSpaces needs no TLB flush. The AddressSpace also records whether its fault handler is busy and how many faults went unhandled; see [fault delivery](../core/thread.md#fault-delivery). Retire does not free the KeyTable's memory.
