# Scheduler

| | |
|---|---|
| Wire type | `0x05` (core) |
| Associated object | A `Thread` authorized to act as a user-space scheduler |
| Status | Contract selected; not implemented |

## Purpose

A `Scheduler` capability marks a Thread that may perform scheduler operations.
Schedulers own the pages containing their scheduling records and explicitly
share those pages with the kernel. There is no publicly visible global DCB
view. Scheduler policy remains in userspace; kernel-only TCB and execution
state remains private.

Scheduler capabilities are derivable to support hierarchical schedulers. The
root scheduler is established during boot before scheduling begins and is the
only scheduler that creates Threads. It may donate Threads to subordinate
schedulers. The kernel must enforce a strict scheduler tree; the exact
creation, donation, and tree-maintenance schemas remain to be specified.

## User-level visible operations

| Op | Name | Selected behavior |
|---|---|---|
| TBD | ShareRegion | Share one Frame page, already mapped into the scheduler's AddressSpace, with the kernel. Conflicting re-share/replacement is rejected. The operation ID, register schema, rights, results, and errors remain open. |

## Shared scheduling records

The kernel locates a Thread's record in a scheduler-declared fixed-stride
table, using a kernel-validated Thread identity/incarnation. The scheduler can
write any bytes in pages it maps. The initial record division starts from the
existing `DcbPage`/TCB division: record bytes are scheduler-writable, while
kernel-only TCB/execution state is not exposed in the shared pages. Exact
field semantics and which transitions the kernel validates remain open.

Table declaration, page capacity, allocation, publication/snapshot semantics,
event-summary indexing, and record retirement/reuse are not yet specified.
Implementations must reject stale Thread identities and must not permit a
record for a retired Thread incarnation to identify a replacement Thread.

## Bootstrap and implementation status

Kickstart establishes the root scheduler before scheduling starts; this
contract does not require a kernel FIFO fallback before then. The kernel's
current FIFO scheduling mechanism remains bootstrap implementation machinery
until scheduler upcalls are implemented. No `Scheduler` ABI wrapper, API
handler, shared-region state, or tree enforcement is implemented yet.

## TODOs

- Define exact ShareRegion argument/result/rights schema and table descriptor.
- Specify and implement root-only Thread creation, Thread donation, and strict
tree enforcement.
- Define record fields, publication/snapshot protocol, event summaries, and
  teardown/reuse rules.
- Connect Thread allocation/retirement and notification/block accounting to
  scheduler-shared records.
- Implement userspace encoding, kernel authorization/object transitions, and
  ABI and target tests as coherent vertical slices.
