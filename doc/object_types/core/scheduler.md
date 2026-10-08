# Scheduler

| | |
|---|---|
| Wire type | `0x05` (core) |
| Backing | None |
| Status | Not implemented |

## Purpose

A Scheduler capability will mark a Thread as a userspace scheduler. A scheduler will share pages with the kernel holding the scheduling records of its Threads, so it can make scheduling decisions without a system call per Thread. Scheduler capabilities will form a tree: the root scheduler creates Threads and can hand them to subordinate schedulers. Invoking a Scheduler capability fails with `UNSUPPORTED_CORE_TYPE`.

Until then, the kernel runs Threads in first-in, first-out order.
