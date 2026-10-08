# ASIDControl

| | |
|---|---|
| Wire type | `0x84` (arch index 4) |
| Backing | None |
| Status | Not implemented |

## Purpose

ASIDControl will control the ASID space as a whole, creating further [ASID pools](asid_pool.md). Today the only pool is the boot pool, and ASIDs are bound to AddressSpaces with [`ASIDPool.Assign`](asid_pool.md#assign). Invoking an ASIDControl capability fails with `UNSUPPORTED_ARCH_TYPE`.
