# Frame

| | |
|---|---|
| Wire type | `0x80` (arch index 0) |
| Backing | Physical memory carved from an Untyped by Retype |
| Operations | Map, Unmap, GetAddress, CleanData, InvalidateData, CleanInvalidateData, UnifyInstruction |

## Purpose

A Frame is a block of physical memory that can be mapped into an AddressSpace: 4 KiB, 2 MiB or 1 GiB (`size_bits` 12, 21 or 30). Components share memory by mapping the same Frame into each of their AddressSpaces; [KeyTable.CopyDerive](../core/key_table.md#copyderive) gives each participant its own Frame capability to map.

A Frame capability tracks at most one mapping at a time. To map the same memory twice, copy the capability first; a copy starts unmapped.

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Map | `x2` target AddressSpace, `x3` virtual address, `x4` rights, `x5` attributes | Requested rights on the Frame, `Map` on the AddressSpace | — |
| 1 | Unmap | — | — | — |
| 2 | GetAddress | — | `Grant` | `x1` physical base, `x2` size in bytes |
| 3 | CleanData | `x2` start offset, `x3` end offset | `Read` | — |
| 4 | InvalidateData | `x2` start offset, `x3` end offset | `Write` | — |
| 5 | CleanInvalidateData | `x2` start offset, `x3` end offset | `Read` | — |
| 6 | UnifyInstruction | `x2` start offset, `x3` end offset | `Read` | — |

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

### Cache maintenance

CleanData, InvalidateData, CleanInvalidateData and UnifyInstruction maintain the caches for bytes `x2` up to, not including, `x3` of the Frame's memory, for sharing it with a device that does not snoop the processor's caches. The kernel works on the memory directly, so the Frame does not have to be mapped. `x4..x7` must be zero.

| Op | Effect |
|---|---|
| CleanData | Writes dirty cache lines back to memory (the Point of Coherency), so a device reads the latest data |
| InvalidateData | Discards cache lines, so the processor next reads what a device wrote; a line only partly inside the range is written back first, so bytes outside the range keep their values |
| CleanInvalidateData | Writes dirty lines back, then discards them |
| UnifyInstruction | Writes dirty lines back to the Point of Unification and invalidates the instruction caches, so code written through a data mapping can run |

The operations act on whole cache lines; the kernel reads the line size from `CTR_EL0`. InvalidateData needs `Write` because discarding a dirty line loses data the way a write does. `FrameKey` wraps them as `clean_data`, `invalidate_data`, `clean_invalidate_data` and `unify_instruction`.

| Error | Cause |
|---|---|
| `InsufficientRights` | The Frame capability lacks the operation's right |
| `InvalidOperation` | `x2 >= x3`, `x3` beyond the Frame's size, a device Frame, or a nonzero unused argument |
| `TypeMismatch` | The key names another kind |

## Implementation

Retype zeroes a Frame carved from ordinary memory before handing it out. A Frame carved from a device Untyped covers hardware registers and is never zeroed or touched. Deleting or moving a Frame capability leaves its mapping in place; unmap it first if the memory should become unreachable.
