# Capabilities completed

Short markers of what is done, grouped by plan phase. Each line says what exists and where it is exercised. Rationale and details are in the [contract](capabilities-contract.md); the history of how each piece landed is in version control.

## Object-types review

- [x] `Domain` split into the arch `AddressSpace` (translation root, ASID, protection boundary) and the core `Thread` (execution, scheduling, pending state); catalogue renumbered, docs regenerated.
- [x] Reserved `ASID` kind replaced by `ASIDControl` (arch index 4, `0x84`); ASIDs are values bound to an AddressSpace.

## Phase 1 — Confirm contracts and support boundaries

- [x] Status matrix confirmed against dispatch; it lives as the catalogue in `doc/object_types/README.md`.
- [x] Ordinary-operation schema template (ID, widths, slot scope, authority, result, blocking, ownership, failure) recorded in the contract.

## Phase 2 — One shared, testable ABI

- [x] Host ABI tests (`just test-object-host`, `host-tests` feature) and host lint (`just clippy-object-host`) wired into `just test`/`just clippy`; the ABI is tested natively on its own architecture.
- [x] One canonical `CoreType`/`ArchType` catalogue (`0x80` arch bit, category-local indices distinct from wire IDs) with exhaustive literal tests over all 256 bytes.
- [x] Full-width operation decoding for every kind (unassigned IDs and high-bit aliases rejected); checked narrowing of slots, sizes and kinds; nonzero unused words rejected.
- [x] `Rights::from_wire` rejects undefined rights bits before authority checks in CopyDerive, Retype and `Frame.Map` (closed a Retype hole that minted bits 6–7).
- [x] Shared status codes 0–34 with one encoder/decoder pair; anything unrepresentable decodes losslessly as `UnknownResponse`; no per-family error spaces.
- [x] Operation schemas and unassigned IDs recorded; strict wire-argument convention (unused words zero).
- [x] User-visible records (`PpcResult`, `ResultSpill`, `ExportRecord`) carry const layout assertions.
- [x] Current-consumer compatibility policy: rebuild everything together; unsupported operations show up as errors.

## Phase 3 — Repair the active syscall/console path

- [x] DebugConsole gated behind `debug_kernel` (handler, grant, wrapper, boot use); its results are propagated and its wrapper no longer prints.
- [x] Explicit caller context: an absent current Thread is rejected, never defaulted to Thread zero.
- [x] Entry checks the exception class and origin; non-SVC EL0 exceptions go to fault delivery; the SVC immediate is ignored.
- [x] No panicking register conversions on the entry path.
- [x] Register preservation: an ordinary invocation writes only `x0..x2`; `x3..x30`, SP and NZCV survive success, rejection and blocking resume (`libkicktest::registers`, `capability-test`, `sync-test`).
- [x] Every wrapper returns kernel results through `decode_syscall_result`; unimplemented operations fail with `InvalidOperation`.

## Phase 4 — Capability storage and Thread lifetime

- [x] Packed 64-bit keys (32-bit incarnation, 32-bit table-relative address); commit-only incarnation advance, retained counters, slot-local exhaustion (`KeySlotExhausted`); statuses 26–28 with reasons and operand index; actual-key bootstrap handoff.
- [x] Guarded key space: variable-size KeyTables (`size_bits` 1–20), per-table guard in the KeyTable capability, `SELF_KEYTABLE` as the guard source, `GuardMismatch` rejection.
- [x] Guarded kernel access: owning `Access` context, short-lived guards, alias-safe pair resolution, single-core execution, no lifetime-erasing casts; `KeyEntry` is kernel-private.
- [x] KeyTable invariants: no counted null inserts, checked bounds, failed insertion returns ownership.
- [x] KeyTable CopyDerive/Move/Delete with separate `DERIVE`/`REMOVE`/`INSTALL` rights, incarnation-checked selectors, vacant destinations, atomic cross-table updates and Move rollback (`test-key-table`).
- [x] Pooled objects carry generation-checked identities (`ObjectPool`, one variable-size carve per pool from the boot Untyped).
- [x] `Thread.Retire` (op 4, `RETIRE`) cancels the Thread's pending waits and frees its slot; `AddressSpace.Retire` (op 1) invalidates its ASID and releases it (`sync-test`, `memory-test`).
- [x] Inert-nucleus boot: Kickstart carves the `Nucleus`, the boot Thread/AddressSpace and table, and installs the boot grants.
- [x] Scheduler and Brand wire IDs registered (`Scheduler = 5`, `Brand = 6`) with no handlers.

## Phase 5 — Memory and safe reclamation

- [x] Buffer is a userspace construct over Frames; no kernel Buffer kind.
- [x] Transactional batch `Untyped.Retype` (validate → reserve → initialize → install → commit watermark): only the unused watermark range, absolute alignment, representable extents, device sources rejected except for Untyped splits (`test-untyped`).
- [x] Retype creates KeyTables, Frames (12/21/30-bit), PageTables, Notifications, EventCounts and Untyped children; Frames and PageTables are zeroed before they become visible.
- [x] Explicit PageTable installation (`PageTable.Map`/`Unmap`); `Frame.Map`/`Unmap`/`GetAddress` with an explicit target AddressSpace, rights ceiling, W^X at one exception level and recorded mapping identity; Copy installs no mapping.
- [x] No intra-AddressSpace physical aliases (`PhysicalAlias`); cross-AddressSpace aliases allowed.
- [x] ASID pool allocation, `ASIDPool.Assign`, ASID-scoped TLB invalidation on unmap, `AddressSpace.Activate` installing the root, non-global TTBR0 leaves (`test-memory`).
- [x] Kernel object pools sized at creation from one watermark carve (`ObjectPool::carve`, `RegionPayload::reserve`).

## Phase 6 — Deferred completion and IPC

- [x] Pending-invocation records with non-wrapping generations, a single terminal transition, bounded FIFO wait queues, and teardown cancellation.
- [x] Blocking waits park a Thread-resident continuation; one kernel stack per core; resume rewrites the transient frame.
- [x] Notification Signal/Wait/Poll (`SEND`/`RECV`, badge or argument bits, one consumer) and EventCount Advance/Await/Read (overflow as `CounterOverflow`, broadcast wakeups), with wrappers and host tests (`test-sync`).
- [x] Kernel-mediated release/acquire for payload publication.
- [x] PPC: `AddressSpace.CreateInvocation` (op 3) with stack extent and headroom, `InvalidStack` 1–12; `Invocation.Call` with `x9` SP, depth-16 Thread-resident stack, `NestingDepth`; `Thread.Return` on `CurrentReturnOnly`, Return faults; register scrubbing and status inheritance (`test-ppc`).
- [x] One KeyTable per AddressSpace, bound at provisioning; source and Bounce run on independent roots and ASIDs.
- [x] Userspace PPC: `ppc_call`/`ppc_return` wrappers, `ppc_export!`, `export::complete_export`, `UnexpectedReturn` (34), `vesper_thread_return_fault` handoff.
- [x] Architectural state: `TPIDR_EL0` per Thread and scrubbed across PPC; `TPIDRRO_EL0`, `CNTKCTL_EL1`, `MDSCR_EL1`, `PMUSERENR_EL0` set at boot; FP/SIMD trapped at EL0 and EL1 (`test-fp-trap`, `just audit-fp-simd`).
- [x] Per-call timestamp in each continuation record (charging inert until a clock exists).
- [x] Queued-rendezvous endpoint example as a userspace composition over Invocation and Notification/EventCount (`test-endpoint`).
- [x] Fault delivery: synchronous upcall into `KeySlot::FAULT_HANDLER`, retry/skip/terminate, unhandled faults park the Thread as `Faulted` (`test-fault`).

## Final integration

- [x] Separately linked EL0 component images, bundled and loaded in place (`libs/image`, `libs/image-build`, `image.toml` manifests).
- [x] Kickstart finds its nucleus image through the manifest.
- [x] `EXECUTE` mappings are executable at exactly one level (PXN/UXN), never RW+X at EL0.
