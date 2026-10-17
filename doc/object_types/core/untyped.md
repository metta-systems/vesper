# Untyped

| | |
|---|---|
| Wire type | `0x01` (core) |
| Backing | A physical memory region of `2^size_bits` bytes, described inline in the capability |
| Operations | Retype |

## Purpose

An Untyped capability grants authority over a region of unallocated physical memory. It is the only way to create kernel objects: `Retype` carves new objects from the region and installs capabilities for them into a KeyTable. The boot Untyped is handed to the first component at `KeySlot::BOOT_UNTYPED` (slot 5).

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Retype | `x2` kind, `x3` size (and guard), `x4` count, `x5` destination table, `x6` first slot, `x7` rights | `Write` on the Untyped, `Install` on the destination table | `x1` key of the first new capability |

### Retype

| Register | Meaning |
|---|---|
| `x2` | Wire type of the objects to create |
| `x3` | Bits 7–0: `size_bits`. Bits 39–8: the new table's guard (KeyTable only). Bits 15–8 for a Frame or an Untyped: the memory-attribute ceiling code (device sources only; 0 inherits the source's). All other bits zero |
| `x4` | Number of objects, 1–256 |
| `x5` | Key of the destination KeyTable |
| `x6` | First destination slot, as a bare index |
| `x7` | Rights for the new capabilities (any subset of `0x7F`) |

Retype creates `count` objects and installs their capabilities into consecutive destination slots starting at `x6`. It succeeds completely or changes nothing.

| Kind | `size_bits` | Result |
|---|---|---|
| KeyTable | 1–20; capacity `2^size_bits` entries | An empty table. The guard must fit in `32 − size_bits` bits and is fixed for the table's lifetime |
| Frame | 12, 21 or 30 (4 KiB, 2 MiB, 1 GiB) | Zeroed physical memory, aligned to its size |
| PageTable | 12 | A zeroed 4 KiB translation table and its kernel record |
| Notification | 0 | A Notification with no pending bits |
| EventCount | 0 | An EventCount at 0 |
| Time | 0 | An empty Time on the creating core: zero budget |
| Thread | 0 | An inert Thread: no AddressSpace or execution state until the root [Scheduler](scheduler.md) configures and starts it |
| AddressSpace | 0 | An inert [AddressSpace](../arch/address_space.md): no KeyTable, translation root or ASID until [`Bind`](../arch/address_space.md#bind), [`PageTable.Map`](../arch/page_table.md#map) and [`ASIDPool.Assign`](../arch/asid_pool.md) |
| Untyped | At least 4 (16 bytes) | A smaller Untyped covering part of the region |

Other kinds fail with `InvalidObjectType`. A device Untyped covers hardware registers: it can be split into smaller device Untypeds or carved into device [Frames](../arch/frame.md), which Retype never zeroes or otherwise touches. Every other kind from a device Untyped fails with `InvalidObjectType`.

| Error | Cause |
|---|---|
| `InvalidObjectType` | The kind cannot be created from memory, or the source is device memory and the kind is neither Untyped nor Frame |
| `InvalidSize` | `size_bits` out of range for the kind; for a KeyTable, a guard too wide for its size or `x3` bits 63–40 set |
| `InvalidFrameSize` | Frame `size_bits` other than 12, 21 or 30 |
| `InvalidOperation` | Count 0 or above 256; `x3` bits above 7 set for a kind other than KeyTable, Frame or Untyped; for a Frame or Untyped, `x3` bits above 15 set or an undefined, `Cacheable` or looser-than-the-source ceiling code, or any ceiling code on a RAM source; a kind, slot or rights word that does not fit its field; undefined rights bits |
| `InsufficientRights` | Missing `Write` on the Untyped or `Install` on the destination |
| `InsufficientMemory` | The objects do not fit in the remaining region |
| `InvalidSlot`, `SlotOccupied`, `KeySlotExhausted` | A destination slot is out of range, occupied, or has used up its incarnations |


### Memory attribute ceilings

A device Untyped carries a ceiling: the loosest memory attribute the [Frames](../arch/frame.md#memory-attributes) carved from it may be mapped with (`DeviceStrict`, `DevicePosted` or `WriteCombining`). Split children inherit it. A Retype of a Frame or an Untyped may lower the ceiling of what it creates: bits 15–8 of `x3` hold the ceiling code (`FrameAttribute` in `libobject`, `UntypedKey::retype_with_ceiling`), and 0 inherits the source's. A ceiling looser than the source's, `Cacheable`, an undefined code, or any ceiling code on a RAM source fails with `InvalidOperation`, before anything is carved. RAM Untypeds have no ceiling: their Frames map `Cacheable` only.

Kickstart gives the register Untypeds `DeviceStrict` and the video-memory Untypeds `WriteCombining`.

## Implementation

Kickstart gives the first component device Untypeds over the `VideoCore` memory the device tree does not list (from the top of RAM to the lowest device register, at most the first gigabyte) and for the hardware registers listed in the device tree: each enabled node's range is rounded out to whole 4 KiB pages, overlapping pages are merged, and each run is split into aligned power-of-two blocks. Pages that hold registers the kernel drives itself, such as the interrupt controller, are never handed out, along with any other device on the same page.

An Untyped tracks a watermark: Retype allocates from the unused part of the region above it and only ever moves it forward. Memory below the watermark is never handed out again, even after the objects created there are retired. Each new object is aligned to its own alignment (a Frame to its size), and the watermark advances in 16-byte steps.

Every object Retype creates takes its memory from the region, so the number of objects a component can create is bounded by the memory it holds. The memory of a KeyTable, Notification, EventCount, Time or Thread holds kernel state that the caller cannot read or map. A PageTable also takes a small kernel record of the same kind, placed after the batch's tables (see [PageTable](../arch/page_table.md#implementation)).
