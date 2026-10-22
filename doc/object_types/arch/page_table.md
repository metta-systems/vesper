# PageTable

| | |
|---|---|
| Wire type | `0x81` (arch index 1) |
| Backing | A 4 KiB table and its kernel record, carved from an Untyped by Retype |
| Operations | Map, Unmap |

## Purpose

A PageTable is one 4 KiB hardware translation table (512 entries). The kernel never creates translation tables on its own: a component builds each AddressSpace's translation tree from PageTables it creates with Retype, so all translation memory is accounted to an Untyped. The tree has four levels; a level-0 table is the AddressSpace's root.

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Map | `x2` parent (AddressSpace or PageTable), `x3` virtual address | `Map` on the parent | — |
| 1 | Unmap | — | — | — |

### Map

Installs the table under the parent named by `x2`:

- **Parent is an AddressSpace:** the table becomes that AddressSpace's root (level 0). `x3` must be 0, and the AddressSpace must have no root yet.
- **Parent is a PageTable:** the table is installed in the parent's entry that covers `x3`, one level below the parent. The parent must itself be installed, must not be a level-3 table, and the entry must be empty.

Install tables from the root downwards: level 0, then levels 1, 2 and 3 as needed. [Frames](frame.md#map) map at level 3 (4 KiB), level 2 (2 MiB) or level 1 (1 GiB).

| Error | Cause |
|---|---|
| `InsufficientRights` | The parent capability lacks `Map` |
| `AlreadyMapped` | This table is already installed, the AddressSpace already has a root, or the parent entry is in use |
| `NotMapped` | The parent PageTable is not installed |
| `InvalidOperation` | Nonzero `x3` for a root, a level-3 parent, mapping a table into itself, or a nonzero unused argument |
| `InvalidObjectType`, `TypeMismatch` | `x2` is neither an AddressSpace nor a PageTable |

### Unmap

Removes the table from its parent. The table must be empty. Unmapping a root detaches it from its AddressSpace and flushes every TLB entry tagged with that AddressSpace's ASID.

| Error | Cause |
|---|---|
| `NotMapped` | The table is not installed |
| `InvalidOperation` | The table still has entries |

## Implementation

Retype zeroes a PageTable before handing it out. For each table it also carves a small kernel record (the table's address, level and where it is installed) from the same Untyped, placed after all the tables of the batch, so the tables stay contiguous and page-aligned and a batch of n tables needs n × 4 KiB plus n records. The format is AArch64 stage 1 with a 4 KiB granule and 48-bit virtual addresses. Unmap leaves the table's memory allocated, so it can be installed again.
