# Frame

| | |
|---|---|
| Wire type | `0x80` (arch index 0) |
| Backing | Physical memory carved from an Untyped by Retype |
| Operations | Map, Unmap, GetAddress |

## Purpose

A Frame is a block of physical memory that can be mapped into an AddressSpace: 4 KiB, 2 MiB or 1 GiB (`size_bits` 12, 21 or 30). Components share memory by mapping the same Frame into each of their AddressSpaces; [KeyTable.CopyDerive](../core/key_table.md#copyderive) gives each participant its own Frame capability to map.

A Frame capability tracks at most one mapping at a time. To map the same memory twice, copy the capability first; a copy starts unmapped.

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Map | `x2` target AddressSpace, `x3` virtual address, `x4` rights, `x5` attributes | Requested rights on the Frame, `Map` on the AddressSpace | — |
| 1 | Unmap | — | — | — |
| 2 | GetAddress | — | `Grant` | `x1` physical base, `x2` size in bytes |

### Map

Maps the Frame at virtual address `x3` in the AddressSpace named by `x2`. The address must be aligned to the Frame's size and below `2^48`. Every translation table on the way down must already be installed with [`PageTable.Map`](page_table.md#map). `x5` must be zero (normal write-back cacheable memory).

`x4` selects the access rights of the mapping. It must include `Read` and be a subset of the Frame capability's rights:

| `x4` | EL0 access | EL1 access |
|---|---|---|
| `Read` | Read | Read |
| `Read + Write` | Read, write | Read, write |
| `Read + Execute` | Read, execute | Read |
| `Read + Write + Execute` | None | Read, write, execute |

No mapping is both writable and executable at the same exception level. A device Frame maps as Device-nGnRnE memory and is never executable: requesting `Execute` fails with `InvalidOperation`.

Within one AddressSpace, a physical byte may be mapped at only one virtual address: Map fails with `PhysicalAlias` if the Frame overlaps memory already mapped there through any capability. Different AddressSpaces may map the same memory at any addresses.

| Error | Cause |
|---|---|
| `InsufficientRights` | `x4` lacks `Read`, asks for rights the Frame lacks, or the AddressSpace capability lacks `Map` |
| `AlreadyMapped` | This Frame capability is already mapped, or the virtual address is in use |
| `MissingIntermediate` | A translation table on the path is missing (`x1` is the address) |
| `PhysicalAlias` | The memory is already mapped in this AddressSpace (`x1` is the conflicting physical base) |
| `InvalidOperation` | Nonzero `x5`, misaligned address, undefined rights bits, `Execute` on a device Frame, or a nonzero unused argument |
| `TypeMismatch` | `x2` is not an AddressSpace |

### Unmap

Removes the Frame's mapping and flushes it from the TLB, so the address is free for a new mapping. Fails with `NotMapped` if the Frame is not mapped.

### GetAddress

Returns the physical base address and size of the Frame. `FrameKey::get_extent` wraps it.

## Implementation

Retype zeroes a Frame carved from ordinary memory before handing it out. A Frame carved from a device Untyped covers hardware registers and is never zeroed or touched. Deleting or moving a Frame capability leaves its mapping in place; unmap it first if the memory should become unreachable.
