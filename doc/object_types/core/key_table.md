# KeyTable

| | |
|---|---|
| Wire type | `0x02` (core) |
| Backing | Memory carved from an Untyped by Retype |
| Status | CopyDerive, Move, Delete |

## Purpose

A KeyTable holds capabilities: `2^size_bits` slots (`size_bits` 1–20, chosen at Retype), each either empty or holding one capability. Every AddressSpace has exactly one KeyTable, shared by all Threads running in it; keys passed in `x0..x7` are looked up there.

A KeyTable capability authorizes managing the entries of that table — copying, moving and deleting them — independently of what the entries themselves authorize. Because every table operation names its tables through keys in the caller's own table, a component can manage any table it holds a capability to.

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | CopyDerive | `x2` source key, `x3` destination table, `x4` destination slot, `x5` rights | `DERIVE` on the invoked table, `INSTALL` on the destination | `x1` new key |
| 1 | Move | `x2` source key, `x3` destination table, `x4` destination slot | `DERIVE` and `REMOVE` on the invoked table, `INSTALL` on the destination | `x1` new key |
| 2 | Delete | `x2` key of the entry to remove | `REMOVE` on the invoked table | — |
| 4 | Revoke | — | — | Not implemented: `INVALID_OPERATION` |

Table rights: bit 0 `DERIVE`, bit 1 `REMOVE`, bit 2 `INSTALL`.

### CopyDerive

Copies the entry named by `x2` (a key in the invoked table) into slot `x4` of the table named by `x3` (a key in the caller's table), with the rights in `x5`. The copy keeps the badge. Its rights must be a subset of the source's; asking for more fails with `INSUFFICIENT_RIGHTS`, and a rights word with bits outside `0x3F` fails with `INVALID_OPERATION`.

CopyDerive and Move accept KeyTable, Frame and DebugConsole entries; other kinds fail with `INVALID_OBJECT_TYPE`. A copied Frame starts unmapped.

### Move

Moves the entry to the destination slot, keeping its rights, badge and state (a Frame keeps its mapping). The source key stops working. Moving an entry onto its own slot fails with `SLOT_OCCUPIED`.

### Delete

Removes the entry. The object it named is unaffected: deleting a capability neither retires the object nor unmaps a Frame. Delete accepts an entry of any kind, including one whose object has been retired.

### Errors

| Error | Cause |
|---|---|
| `INSUFFICIENT_RIGHTS` | A table right is missing, or CopyDerive asks for rights the source lacks |
| `INVALID_OBJECT_TYPE` | CopyDerive or Move of a kind that cannot be copied |
| `INVALID_OPERATION` | Undefined rights bits, nonzero unused argument |
| `INVALID_SLOT`, `SLOT_OCCUPIED` | Destination slot out of range or occupied (including a Move onto its own slot) |
| `KEY_SLOT_EXHAUSTED` | The destination slot has used up its incarnations |
| `INVALID_KEY`, `INCONSISTENT_KEY` | A key argument failed lookup; see [key errors](../README.md#key-errors) |

Every failure leaves both tables unchanged.

## Keys and incarnations

Each slot counts how many capabilities it has held. Installing a capability increments the counter and stamps it into the new key as its incarnation; deleting or moving the capability leaves the counter as it is. A key whose incarnation no longer matches the slot fails with `INCONSISTENT_KEY`, so an old key never reaches a later occupant of the same slot. A slot whose counter reaches `u32::MAX` accepts no further installs (`KEY_SLOT_EXHAUSTED`); its current entry stays usable and deletable.

Each table also has a guard, chosen at Retype and fixed for the table's lifetime. Every key issued for the table carries the guard above the slot index, so a key presented to a different table fails with `GuardMismatch`.

## Well-known slots

| Slot | Constant | Contents |
|---:|---|---|
| 0 | `KeySlot::NULL` | Always empty |
| 1 | `KeySlot::THREAD_RETURN` | The return key used by [`Thread.Return`](thread.md#return); installed when the AddressSpace is created |
| 2 | `KeySlot::SELF_ADDRESS_SPACE` | This AddressSpace |
| 3 | `KeySlot::PARENT_THREAD` | The parent Thread |
| 4 | `KeySlot::SELF_KEYTABLE` | This KeyTable. The kernel reads the table's guard from it; invocations fail if it is missing or names another table |
| 5 | `KeySlot::BOOT_UNTYPED` | Boot Untyped (first component) |
| 14 | `KeySlot::BOOT_ASID_POOL` | Boot ASID pool (first component) |
| 15 | `KeySlot::DEBUG_CONSOLE` | Debug console (debug kernels) |
| 16 | `KeySlot::FAULT_HANDLER` | This AddressSpace's fault handler, an Invocation; see [fault delivery](thread.md#fault-delivery) |

## Implementation

A table of `2^size_bits` entries takes a 32-byte header, 64 bytes per entry and 4 bytes per incarnation counter, rounded up to 32-byte alignment: 17,440 bytes for 256 entries. A new table is all empty slots.
