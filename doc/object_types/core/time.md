# Time

| | |
|---|---|
| Wire type | `0x04` (core) |
| Backing | None |
| Status | Not implemented |

## Purpose

A Time capability will represent a CPU-time budget: the right to run for a bounded amount of time. Budgets will be split, merged and donated between components, so that userspace schedulers can hand out CPU time the way Untyped hands out memory. Invoking a Time capability fails with `UNSUPPORTED_CORE_TYPE`.

## Planned changes

Operations Donate (0), Split (1), Merge (2) and Query (3), together with timer support in the kernel.
