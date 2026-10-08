# IOPort

| | |
|---|---|
| Wire type | `0x86` (arch index 6) |
| Backing | None |
| Status | x86 only; not available on AArch64 |

## Purpose

An IOPort capability grants access to a range of x86 I/O ports (`in`/`out` instructions). AArch64 has no I/O ports, so the kind does not exist there; invoking it fails with `UNSUPPORTED_ARCH_TYPE`.
