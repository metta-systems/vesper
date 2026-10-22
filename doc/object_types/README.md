# Object types

Reference for every kind of kernel object a Vesper component can hold a capability to: what it is for, which operations it supports, and how to invoke them. One page per kind.

## Wire encoding

An object type is one byte. Bit 7 selects the category; bits 6–0 hold the index within it.

- Core kinds: `0x00..=0x7f`.
- Architecture kinds: `0x80 | index`, `0x80..=0xff`.

## Catalogue

### Core kinds

| Kind | Wire | Page | Operations |
|---|---:|---|---|
| Null | `0x00` | [null.md](core/null.md) | Every invocation fails |
| Untyped | `0x01` | [untyped.md](core/untyped.md) | Retype |
| KeyTable | `0x02` | [key_table.md](core/key_table.md) | CopyDerive, Move, Delete |
| Thread | `0x03` | [thread.md](core/thread.md) | Return, Suspend, Resume, Retire |
| Time | `0x04` | [time.md](core/time.md) | Transfer, Merge, Query |
| Scheduler | `0x05` | [scheduler.md](core/scheduler.md) | ShareRegion, ConfigureThread, StartThread, AssignRecord, BindEventCount, BindDispatcher, Dispatch, ConfigureActivation, ArmTimer, Adopt, MoveThread |
| Invocation | `0x06` | [invocation.md](core/invocation.md) | Call, CallWithKey |
| Notification | `0x07` | [notification.md](core/notification.md) | Signal, Await, Poll |
| EventCount | `0x08` | [event_count.md](core/event_count.md) | Advance, Await, Read |
| WaitSet | `0x09` | [wait_set.md](core/wait_set.md) | AddMember, UpdateMember, RemoveMember, Await |
| IRQControl | `0x0a` | [irq_control.md](core/irq_control.md) | Issue |
| IRQHandler | `0x0b` | [irq_handler.md](core/irq_handler.md) | SetNotification, Clear, Ack, Retire, Query |
| DebugConsole | `0x7f` | [debug_console.md](core/debug_console.md) | Write (debug kernels only) |

Indices 12–126 are unassigned.

### Architecture kinds

| Kind | Wire | Page | Operations on AArch64 |
|---|---:|---|---|
| Frame | `0x80` | [frame.md](arch/frame.md) | Map, Unmap, GetAddress, cache maintenance |
| PageTable | `0x81` | [page_table.md](arch/page_table.md) | Map, Unmap |
| AddressSpace | `0x82` | [address_space.md](arch/address_space.md) | Activate, Retire, Bind, CreateInvocation |
| ASIDPool | `0x83` | [asid_pool.md](arch/asid_pool.md) | Assign |
| ASIDControl | `0x84` | [asid_control.md](arch/asid_control.md) | Not implemented |
| IOSpace | `0x85` | [io_space.md](arch/io_space.md) | Not implemented |
| IOPort | `0x86` | [io_port.md](arch/io_port.md) | x86 only; not available on AArch64 |

Indices 7–127 are unassigned. Invoking a kind that is not implemented fails with `UnsupportedCoreType` or `UnsupportedArchType`.

## Invocation model

### Registers

Every operation is one `svc` instruction. The immediate is ignored; the userspace wrappers use `svc #0`.

| Direction | Register | Meaning |
|---|---|---|
| In | `x0` | Key of the invoked capability |
| In | `x1` | Operation number |
| In | `x2..x7` | Six operation arguments |
| Out | `x0` | Status; zero is success |
| Out | `x1`, `x2` | Two result words on success, error details on failure |

Argument words an operation does not use must be zero; a nonzero unused word fails with `InvalidOperation`. Exceptions: `Thread.Return` ignores `x4..x7`; `Invocation.Call` clobbers `x9`; `Invocation.CallWithKey` also reads `x8` (the key to lend); `AddressSpace.CreateInvocation` also reads `x5` (the stack pool handle) and `x8` (the key-placement window).

An invocation writes only `x0..x2`. Every other register, SP and the NZCV flags are returned unchanged, whether the operation succeeds, fails, or blocks and resumes later. `Invocation.Call` and `Thread.Return` move the Thread between address spaces and have their own register rules; see [Invocation](core/invocation.md#register-state).

### Keys

A key is a 64-bit value that names one capability in the caller's KeyTable:

| Bits | Field |
|---|---|
| 63–32 | Incarnation: which occupant of the slot this key names |
| 31–`size_bits` | The table's guard |
| `size_bits`−1–0 | Slot index |

`size_bits` is the table's capacity exponent (a 256-entry table has `size_bits = 8`). A key is valid only while the slot still holds the same occupant; see [KeyTable](core/key_table.md#keys-and-incarnations).

### Status codes

| Status | Name | `x1` | `x2` |
|---:|---|---|---|
| 0 | `Success` | result | result |
| 1 | `Unknown` | 0 | 0 |
| 2 | `NullCapability` | 0 | 0 |
| 3 | `InvalidThread` | 0 | 0 |
| 4 | `InvalidAddressSpace` | 0 | 0 |
| 5 | `InvalidPointer` | 0 | 0 |
| 6 | `InsufficientRights` | 0 | 0 |
| 7 | `NotMapped` | 0 | 0 |
| 8 | `AlreadyMapped` | 0 | 0 |
| 9 | `InvalidOperation` | 0 | 0 |
| 10 | `AsidPoolExhausted` | 0 | 0 |
| 11 | `NoAsidAssigned` | 0 | 0 |
| 12 | `InvalidSlot` | slot | 0 |
| 13 | `EmptySlot` | slot | 0 |
| 14 | `SlotOccupied` | slot | 0 |
| 15 | `NotCoreType` | wire type | 0 |
| 16 | `UnknownCoreType` | type byte | 0 |
| 17 | `UnsupportedCoreType` | core index | 0 |
| 18 | `NotArchType` | wire type | 0 |
| 19 | `UnknownArchType` | type byte | 0 |
| 20 | `UnsupportedArchType` | arch index | 0 |
| 21 | `InvalidObjectType` | wire type | 0 |
| 22 | `TypeMismatch` | expected wire type | found wire type |
| 23 | `InsufficientMemory` | 0 | 0 |
| 24 | `PoolExhausted` | 0 | 0 |
| 25 | `InvalidSize` | size | 0 |
| 26 | `InvalidFrameSize` | size | 0 |
| 27 | `InvalidKey` | submitted key | reason \| (argument register << 8) |
| 28 | `InconsistentKey` | submitted key | reason \| (argument register << 8) |
| 29 | `KeySlotExhausted` | slot | 0 |
| 30 | `MissingIntermediate` | virtual address | 0 |
| 31 | `PhysicalAlias` | physical base of the conflicting mapping | 0 |
| 32 | `CounterOverflow` | 0 | 0 |
| 33 | `InvalidStack` | offending value | reason, see [AddressSpace](arch/address_space.md#stack-pools) |
| 34 | `NestingDepth` | saved continuations | 0 |
| 35 | `UnexpectedReturn` | local `x1` | local `x2` |
| 36 | `InsufficientBudget` | the source Time's remaining budget | 0 |
| 37 | `IrqLineActive` | the line | 0 |
| 38 | `KeyNotAccepted` | 0 | 0 |
| 39 | `NoPlacementSlot` | 0 | 0 |
| 40 | `NotBound` | 0 | 0 |
| 41 | `InvalidSelfReference` | the self slot (4, `SELF_KEYTABLE`) | the key that led to the table, see [KeyTable](core/key_table.md#keys-and-incarnations) |
| 42 | `StaleGeneration` | the current generation of the Thread or stack pool slot | 0 |
| 43 | `TimedOut` | the EventCount value (0 for a Notification) | 0 |
| 44 | `Watched` | 0 | 0 |
| 45 | `MemberRetired` | the WaitSet slot | 0 |
| 46 | `AlreadyBadged` | 0 | 0 |
| 47 | `NoFreeStack` | the pool's stack count | 0 |

`decode_syscall_result` in `libs/object/src/lib.rs` turns these into `CapError`. A status it does not recognize, or a known status with malformed details, decodes as `CapError::UnknownResponse` with all three words kept.

### Key errors

Any key argument can fail lookup. Lookup checks, in this order:

| Check | Error | Reason |
|---|---|---|
| Incarnation is nonzero | `InvalidKey` | 1 `ZeroIncarnation` |
| Guard matches the table's guard | `InvalidKey` | 4 `GuardMismatch` |
| Slot index is within the table's capacity | `InvalidKey` | 2 `SlotOutOfRange` |
| A capability was ever installed in the slot | `InvalidKey` | 3 `NeverIssued` |
| The slot still holds the same occupant | `InconsistentKey` | 1 `SlotIncarnationMismatch` |
| That occupant was not deleted or moved | `InconsistentKey` | 2 `CapabilityInvalidated` |

`x2` bits 15–8 hold the index of the argument register that carried the failing key (`0` for `x0`, `8` for the key `CallWithKey` lends in `x8`).

When an operation uses the object a capability names, it checks after the kind and rights checks that the object still exists. If the object was retired, the operation fails with `InconsistentKey`, reason 3 `ObjectRetired`, against the key that named it. An [Invocation](core/invocation.md#call) whose target AddressSpace was retired fails this way on the Invocation key.

### Dispatch

```mermaid
flowchart TD
    A["svc"] --> B["Check exception class and origin"]
    B --> AS["Find the current Thread's AddressSpace"]
    AS --> KT["Resolve its KeyTable and the table guard"]
    KT --> C["Look up the key in x0"]
    C --> D{"Architecture kind?"}
    D -- "no" --> E["Core handler"]
    D -- "yes" --> F["Architecture handler"]
    E --> G["Untyped, KeyTable, Thread, Time, Scheduler,<br/>Invocation, Notification, EventCount, DebugConsole"]
    E --> I["Null → NULL_CAPABILITY"]
    E --> J["IRQControl, IRQHandler →<br/>UNSUPPORTED_CORE_TYPE"]
    F --> L["Frame, PageTable, AddressSpace, ASIDPool"]
    F --> M["ASIDControl, IOSpace, IOPort →<br/>UNSUPPORTED_ARCH_TYPE"]
```

### Blocking and address-space switches

A blocking operation (`Notification.Await`, `EventCount.Await`, `WaitSet.Await`) does not return to the caller until it completes. The kernel saves the caller's registers in its Thread, runs the next runnable Thread, and later resumes the caller with the result in `x0..x2`. `AddressSpace.Activate`, scheduling, `Invocation.Call` and `Thread.Return` switch the hardware translation context (`TTBR0_EL1` and the ASID) on the way back to the caller. A synchronous exception from EL0 other than `svc` is a fault, delivered to the fault handler of the faulting AddressSpace; see [Thread](core/thread.md#fault-delivery).
