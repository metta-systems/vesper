# Untyped

| | |
|---|---|
| Wire type | `0x01` (core) |
| Backing | A physical memory region of `2^size_bits` bytes, described inline in the capability |
| Status | Retype |

## Purpose

An Untyped capability grants authority over a region of unallocated physical memory. It is the only way to create kernel objects: `Retype` carves new objects from the region and installs capabilities for them into a KeyTable. The boot Untyped is handed to the first component at `KeySlot::BOOT_UNTYPED` (slot 5).

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Retype | `x2` kind, `x3` size (and guard), `x4` count, `x5` destination table, `x6` first slot, `x7` rights | `WRITE` on the Untyped, `INSTALL` on the destination table | `x1` key of the first new capability |

### Retype

| Register | Meaning |
|---|---|
| `x2` | Wire type of the objects to create |
| `x3` | Bits 7–0: `size_bits`. Bits 39–8: the new table's guard (KeyTable only). All other bits zero |
| `x4` | Number of objects, 1–256 |
| `x5` | Key of the destination KeyTable |
| `x6` | First destination slot, as a bare index |
| `x7` | Rights for the new capabilities (any subset of `0x3F`) |

Retype creates `count` objects and installs their capabilities into consecutive destination slots starting at `x6`. It succeeds completely or changes nothing.

| Kind | `size_bits` | Result |
|---|---|---|
| KeyTable | 1–20; capacity `2^size_bits` entries | An empty table. The guard must fit in `32 − size_bits` bits and is fixed for the table's lifetime |
| Frame | 12, 21 or 30 (4 KiB, 2 MiB, 1 GiB) | Zeroed physical memory, aligned to its size |
| PageTable | 12 | A zeroed 4 KiB translation table |
| Notification | 0 | A Notification with no pending bits |
| EventCount | 0 | An EventCount at 0 |
| Untyped | At least 4 (16 bytes) | A smaller Untyped covering part of the region |

Other kinds fail with `INVALID_OBJECT_TYPE`. A device Untyped can only be split into smaller device Untypeds; every other kind from a device Untyped fails with `INVALID_OBJECT_TYPE`.

| Error | Cause |
|---|---|
| `INVALID_OBJECT_TYPE` | The kind cannot be created from memory, or the source is device memory |
| `INVALID_SIZE` | `size_bits` out of range for the kind; for a KeyTable, a guard too wide for its size or `x3` bits 63–40 set |
| `INVALID_FRAME_SIZE` | Frame `size_bits` other than 12, 21 or 30 |
| `INVALID_OPERATION` | Count 0 or above 256; `x3` bits above 7 set for a kind other than KeyTable; a kind, slot or rights word that does not fit its field; undefined rights bits |
| `INSUFFICIENT_RIGHTS` | Missing `WRITE` on the Untyped or `INSTALL` on the destination |
| `INSUFFICIENT_MEMORY` | The objects do not fit in the remaining region |
| `POOL_EXHAUSTED` | No free Notification, EventCount or PageTable slot in the kernel pool |
| `INVALID_SLOT`, `SLOT_OCCUPIED`, `KEY_SLOT_EXHAUSTED` | A destination slot is out of range, occupied, or has used up its incarnations |

## Implementation

An Untyped tracks a watermark: Retype allocates from the unused part of the region above it and only ever moves it forward. Memory below the watermark is never handed out again, even after the objects created there are retired. Each new object is aligned to its own alignment (a Frame to its size), and the watermark advances in 16-byte steps.

KeyTables, Frames, PageTables and child Untypeds take their memory from the region. Notifications and EventCounts take no memory from the region; each takes a slot in a kernel pool created at boot. A PageTable also takes a slot in the PageTable pool.
