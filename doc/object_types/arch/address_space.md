# AddressSpace

| | |
|---|---|
| Wire type | `0x82` (arch index 2) |
| Backing | Kernel record carved from an Untyped by [`Untyped.Retype`](../core/untyped.md#retype) |
| Operations | Activate, Retire, Bind, CreateInvocation, CreateStackPool, DestroyStackPool |

## Purpose

An AddressSpace is a protection domain: a translation root, the ASID bound to it, and one KeyTable shared by every Thread running in it. Threads in different AddressSpaces cannot see each other's memory unless both map the same Frame. A Thread runs in one AddressSpace at a time and moves between them with [Invocation](../core/invocation.md) calls.

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Activate | — | `Map` | — |
| 1 | Retire | — | `Retire` | — |
| 2 | Bind | `x2` KeyTable | `Map` on the AddressSpace, `Install` on the KeyTable | — |
| 3 | CreateInvocation | `x2` entry, `x3` destination table, `x4` destination slot, `x5` stack pool handle, `x8` key-placement window | `Grant` on the AddressSpace, `Install` on the destination | `x1` new Invocation key |
| 4 | CreateStackPool | `x2` base, `x3` bytes per stack, `x4` stack count | `Grant` | `x1` pool handle |
| 5 | DestroyStackPool | `x2` pool handle | `Grant` | — |

### Activate

Installs this AddressSpace's translation root and ASID into the hardware (`TTBR0_EL1`). Only the caller's own AddressSpace can be activated. It needs a root ([`PageTable.Map`](page_table.md#map)) and an ASID ([`ASIDPool.Assign`](asid_pool.md)) first. Activating an already active AddressSpace has no further effect.

| Error | Cause |
|---|---|
| `InsufficientRights` | The capability lacks `Map` |
| `NotBound` | No KeyTable is bound ([Bind](#bind)) |
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

### Bind

Binds the KeyTable named by `x2` to this AddressSpace: Threads running in it resolve their keys through that table. [`Untyped.Retype`](../core/untyped.md#retype) creates an AddressSpace with no KeyTable, and Bind can be done once; the binding lasts for the AddressSpace's lifetime. Several AddressSpaces may share one KeyTable.

Before Bind, install a capability to the table itself in its slot 4 (`SELF_KEYTABLE`): the kernel reads the table's guard from it on every invocation. Bind installs the [return key](../core/thread.md#return) in slot 1 and nothing else, so the caller also installs the AddressSpace's own capability in slot 2 (`SELF_ADDRESS_SPACE`) if its Threads need one.

Until it is bound, an AddressSpace cannot be activated, cannot create Invocations and cannot receive Threads ([`Scheduler.ConfigureThread`](../core/scheduler.md#configurethread)); these fail with `NotBound`. Its translation can be set up before binding: [`PageTable.Map`](page_table.md#map), [`Frame.Map`](frame.md#map) and [`ASIDPool.Assign`](asid_pool.md) work on it.

| Error | Cause |
|---|---|
| `InsufficientRights` | The AddressSpace capability lacks `Map`, or the KeyTable capability lacks `Install` |
| `AlreadyMapped` | A KeyTable is already bound |
| `TypeMismatch` | `x2` is not a KeyTable capability |
| `InvalidSelfReference` | Slot 4 of the table does not hold a capability to the table itself (`x1` = 4, `x2` = the table key) |
| `InconsistentKey` | The AddressSpace was retired (`ObjectRetired`) |
| `SlotOccupied`, `InvalidOperation` | Slot 1 of the table holds another capability, or once held one |
| `InvalidOperation` | A nonzero `x3..x7` |

### CreateInvocation

Creates an [Invocation](../core/invocation.md) that calls `x2` in this AddressSpace on stacks from the pool `x5`, and installs it into slot `x4` of the table named by `x3`. The new capability carries the `Call` and `Badge` rights: its holder can call it and [mint badged copies](../core/key_table.md#setting-a-badge), and a holder that does not need to mint copies it with `Badge` masked off. The entry address must be nonzero (`InvalidPointer`). `x6` and `x7` must be zero, and so must bits 63:40 of `x5` (`InvalidOperation`). The pool is checked at each call, so a Invocation can be created before the pool it names is destroyed or after.

`x8` is the key-placement window that [`CallWithKey`](../core/invocation.md#callwithkey) uses: bits 31:0 are the first slot and bits 63:32 the number of slots, in this AddressSpace's own KeyTable. Zero means the Invocation accepts no lent key. A window that does not fit the table fails with `InvalidSlot`, naming its first slot. If the window is nonzero and the KeyTable does not hold a capability to itself in slot 4, CreateInvocation fails with `InvalidSelfReference` (`x1` = 4, `x2` = the AddressSpace key). The slots need not be vacant when the Invocation is created.

An AddressSpace with no KeyTable bound fails with `NotBound`. Failures leave the destination table unchanged.

```mermaid
flowchart TD
    subgraph Caller["Caller (holds AddressSpace key + destination table key)"]
        A["AddressSpaceKey.create_invocation_with_window(...)"]
    end

    A -->|"SVC on AddressSpace key<br/>op = CreateInvocation"| R

    subgraph Regs["Register schema"]
        R["x2 function_address (entry)<br/>x3 destination KeyTable key<br/>x4 vacant destination slot<br/>x5 stack pool handle<br/>x6, x7 zero<br/>x8 placement window (first, count)"]
    end

    R --> K1{"GRANT right on<br/>AddressSpace key?"}
    K1 -- no --> E1["InsufficientRights"]
    K1 -- yes --> K2{"INSTALL right on<br/>destination KeyTable?"}
    K2 -- no --> E1
    K2 -- yes --> K3{"destination slot vacant?"}
    K3 -- no --> E2["slot error"]
    K3 -- yes --> K4{"x6, x7 zero, handle without<br/>reserved bits?<br/>window inside table capacity?"}
    K4 -- no --> E3["invalid argument error"]
    K4 -- yes --> N

    subgraph Entry["New KeyEntry installed in destination table (64 bytes)"]
        N["obj_type = Invocation<br/>rights = CALL | BADGE<br/>badge = 0<br/>payload:<br/>function_address<br/>address_space_address + generation<br/>pool slot + generation<br/>placement_first / placement_count"]
    end

    N --> RET["Returns destination-table-local RawKey"]

    RET --> D1["Client may CopyDerive with a badge<br/>(needs BADGE right)"]
    RET --> D2["CopyDerive masking BADGE off<br/>gives a CALL-only key"]

    D1 --> CALL["Invocation.Call / CallWithKey<br/>target runs at function_address in the<br/>AddressSpace, on a stack from the pool;<br/>badge arrives in x1, lent key in x0"]
    D2 --> CALL
```

### Stack pools

A stack pool is the set of stacks that calls into this AddressSpace run on. It is `count` stacks (1 to 64) of `stack_bytes` each, back to back from `base`: stack `i` is `[base + i * stack_bytes, base + (i + 1) * stack_bytes)`, and a call on it starts with `SP` at its end. An AddressSpace holds up to 16 pools. A pool is named by a handle, the slot in bits 7:0 and the slot's generation in bits 39:8; a handle is a number and needs `Grant` on the AddressSpace key to use.

The kernel never reads or writes the stack memory and does not check that it is mapped. The usual layout is `[guard][stack]…[stack][guard]`: `stack_bytes` includes an unmapped guard region at the bottom of each unit, and a last guard follows the pool.

**CreateStackPool** fails with `InvalidStack` (status 33; offending value in `x1`, reason in `x2`) and with `PoolExhausted` when all 16 slots are taken. `x5..x7` must be zero (`InvalidOperation`). The checks run in the order of the table.

| Reason | Name | Condition | `x1` |
|---:|---|---|---|
| 6 | `StackCountOutOfRange` | `count` is zero or above 64 | `count` |
| 5 | `StackBytesTooSmall` | `stack_bytes` below 16 | `stack_bytes` |
| 4 | `StackBytesMisaligned` | `stack_bytes` not a multiple of 16 | `stack_bytes` |
| 3 | `BaseMisaligned` | `base` not 16-byte aligned | `base` |
| 1 | `BaseOutsideUserRange` | `base` at or beyond `2^48` | `base` |
| 2 | `EndOutsideUserRange` | `base + count * stack_bytes` beyond `2^48` or overflowing | the end, or `stack_bytes` on overflow |

**DestroyStackPool** fails with `InvalidOperation` while any stack of the pool is in use, and with `StaleGeneration` for a handle that names no pool. Afterwards the handle and every Invocation that names the pool fail with `StaleGeneration`, and the slot makes a new pool under a new generation.

## Implementation

`Untyped.Retype` creates AddressSpaces (`size_bits` 0) from an Untyped's memory; the boot AddressSpace is created and bound by the boot code. An AddressSpace's translation-table leaves are tagged with its ASID, so switching AddressSpaces needs no TLB flush. The AddressSpace holds its stack pool table inline, so pools allocate nothing. It also records whether its fault handler is busy and how many faults went unhandled; see [fault delivery](../core/thread.md#fault-delivery). Retire does not free the KeyTable's memory.
