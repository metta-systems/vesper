# KeyTable

| | |
|---|---|
| Wire type | `0x02` (core) |
| Backing | Memory carved from an Untyped by Retype |
| Operations | CopyDerive, Move, Delete |

## Purpose

A KeyTable holds capabilities: `2^size_bits` slots (`size_bits` 1–20, chosen at Retype), each either empty or holding one key. Every AddressSpace has exactly one KeyTable, shared by all Threads running in it.

A KeyTable capability authorizes managing the entries of that table — copying, moving and deleting them — independently of what the entries themselves authorize. Because every table operation names its tables through keys in the caller's own table, a component can manage any table it holds a capability to.

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | CopyDerive | `x2` source key, `x3` destination table, `x4` destination slot, `x5` rights | `Derive` on the invoked table, `Install` on the destination | `x1` new key |
| 1 | Move | `x2` source key, `x3` destination table, `x4` destination slot | `Derive` and `Remove` on the invoked table, `Install` on the destination | `x1` new key |
| 2 | Delete | `x2` key of the entry to remove | `Remove` on the invoked table | — |
| 4 | Revoke | — | — | Not implemented: `InvalidOperation` |

Table rights: bit 0 `Derive`, bit 1 `Remove`, bit 2 `Install`.

### CopyDerive

Copies the entry named by `x2` (a key in the invoked table) into slot `x4` of the table named by `x3` (a key in the caller's table), with the rights in `x5`. The copy keeps the badge. Its rights must be a subset of the source's; asking for more fails with `InsufficientRights`, and a rights word with bits outside `0x3F` fails with `InvalidOperation`.

CopyDerive and Move accept `KeyTable`, `Frame` and `DebugConsole` entries; other kinds fail with `InvalidObjectType`. A copied Frame starts unmapped.

### Move

Moves the entry to the destination slot, keeping its rights, badge and state (a Frame keeps its mapping). The source key stops working. Moving an entry onto its own slot fails with `SlotOccupied`.

### Delete

Removes the entry. The object it named is unaffected: deleting a capability neither retires the object nor unmaps a Frame. Delete accepts an entry of any kind, including one whose object has been retired.

### Errors

| Error | Cause |
|---|---|
| `InsufficientRights` | A table right is missing, or CopyDerive asks for rights the source lacks |
| `InvalidObjectType` | CopyDerive or Move of a kind that cannot be copied |
| `InvalidOperation` | Undefined rights bits, nonzero unused argument |
| `InvalidSlot`, `SlotOccupied` | Destination slot out of range or occupied (including a Move onto its own slot) |
| `KeySlotExhausted` | The destination slot has used up its incarnations |
| `InvalidKey`, `InconsistentKey` | A key argument failed lookup; see [key errors](../README.md#key-errors) |

Every failure leaves both tables unchanged.

## Keys and incarnations

Each slot counts how many capabilities it has held. Installing a capability increments the counter and stamps it into the new key as its incarnation; deleting or moving the capability leaves the counter as it is. A key whose incarnation no longer matches the slot fails with `InconsistentKey`, so an old key never reaches a later occupant of the same slot. A slot whose counter reaches `u32::MAX` accepts no further installs (`KeySlotExhausted`); its current entry stays usable and deletable.

Each table also has a guard, chosen at Retype and fixed for the table's lifetime. Every key issued for the table carries the guard above the slot index, so a key presented to a different table fails with `GuardMismatch`.

## Implementation

A table of `2^size_bits` entries takes a 32-byte header, 64 bytes per entry and 4 bytes per incarnation counter, rounded up to 32-byte alignment: 17,440 bytes for 256 entries. A new table is all empty slots.

## Planned changes

In seL4, most capability types can be copied or minted, but some are explicitly non-derivable.
Both operations use the same underlying capability derivation mechanism:

- seL4_CNode_Copy creates a capability referring to the same kernel object, potentially with reduced rights.
- seL4_CNode_Mint does the same, but additionally allows modifying capability-specific metadata, such as badges or guard values.

**Neither operation creates a new kernel object.**

| Capability type | Copy | Mint | Notes |
|---|:---:|:---:|---|
| Untyped | ✅ | ✅ | Subject to untyped derivation constraints |
| Endpoint | ✅ | ✅ | Mint can assign a badge |
| Notification | ✅ | ✅ | Mint can assign a badge |
| CNode | ✅ | ✅ | Mint can modify guard and guard size |
| TCB | ✅ | ✅ | No configurable rights |
| Frame / Page | ✅ | ✅ | Rights can be restricted |
| Page Table / VSpace objects | ✅ | ✅ | Architecture-dependent |
| ASID Pool | ✅ | ✅ | |
| IRQHandler | ✅ | ✅ | |
| IRQControl | ❌ | ❌ | Must be moved, not duplicated |
| Domain | ❌ | ❌ | Singleton control capability |
| ASIDControl | ❌ | ❌ | Singleton control capability |
| SchedControl (MCS) | ❌ | ❌ | Singleton scheduling authority |
| SchedContext (MCS) | ✅ | ✅ | |
| Reply (MCS) | ❌ | ❌ | Reply objects exist, but their caps cannot be duplicated |
| Reply (non-MCS) | ❌ | ❌ | Special one-shot reply semantics |
| Null / Zombie | ❌ | ❌ | Not derivable |
