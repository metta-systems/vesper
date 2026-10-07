# Scheduler

| | |
|---|---|
| Wire type | `0x05` (core) |
| Associated object | A `Thread` authorized to act as a userspace scheduler |
| Status | Contract direction recorded; not implemented (dispatch returns `UnsupportedCoreType`) |

## Purpose

A `Scheduler` capability marks a Thread that may perform scheduler
operations. Scheduling policy lives in userspace: a scheduler owns the pages
holding its Threads' scheduling records and shares them with the kernel
explicitly. There is no global DCB view, and kernel-only Thread and execution
state stays private.

Scheduler capabilities derive into a strict tree for hierarchical scheduling.
The root scheduler, established at boot, is the only one that creates Threads;
it may donate Threads to subordinate schedulers.

## User-level visible operations

| Op | Name | Intended behavior |
|---|---|---|
| TBD | ShareRegion | Share one Frame page, already mapped into the scheduler's `AddressSpace`, with the kernel; a conflicting re-share or replacement is rejected |

No operation ID, register schema, rights or error set is selected yet.

## Kernel-level implementation details

- Nothing is implemented: no ABI wrapper, handler, shared-region state or tree
  enforcement.
- The kernel's FIFO runnable queue is bootstrap machinery that stands in until
  scheduler upcalls exist.
- Intended record lookup: a scheduler-declared fixed-stride table, indexed by
  a kernel-validated Thread identity and incarnation. Record bytes are
  scheduler-writable; a record for a retired Thread incarnation must never
  identify its replacement.

## Sidenotes

- The initial record layout starts from the existing `DcbPage`/TCB division:
  scheduler-visible fields in the shared pages, kernel-only state private.

## TODOs

- ShareRegion's schema and the table descriptor — D5.
- Root-only Thread creation, Thread donation and strict tree enforcement.
- Record fields, publication/snapshot protocol, event summaries, and
  teardown/reuse rules.
- Connecting Thread allocation/retirement, wait accounting and per-call PPC
  time attribution to the shared records.
