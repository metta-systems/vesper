# Capability implementation plan

The design authority is [Nucleus capabilities: design contracts](nucleus_capabilities.md). This checklist turns those contracts into dependency-ordered work across `libs/object`, `libs/syscall`, `kernel/nucleus/src/api`, `kernel/nucleus/src/objects`, and the nucleus entry/scheduler/backend code.

This is a TODO list, not a claim of implementation. The reference and initial checklist have been created; approval, reconciliation, and code validation remain work. Mark an item `- [x]` only after its stated outcome is implemented/reviewed and the relevant validation has actually passed. Record blocked or unrun validation rather than checking it off.

## Working rules

- Select a small, explicitly scoped item or coherent group of items; do not execute the entire backlog merely because this file exists.
- Read the reference and relevant decision-register entries first. Resolve prerequisite decisions with the maintainer; do not silently promote a recommendation to a contract.
- Inspect module declarations, feature/target gates, call sites, and current local edits. Distinguish excluded sketches from reachable behavior.
- Change a contract here and in the reference before intentionally implementing a different ABI or semantic model.
- Complete the affected shared definitions, client encoding/decoding, kernel authorization, state transitions, and tests together. Unsupported operations must remain explicit errors.
- Preserve user work. This repository uses JJ: no raw Git, commits, history changes, new changes/branches, or pushes by default. Version-control mutation requires an explicit request.
- Use **`just` for project build, test, formatting, and lint workflows**. Read the current `Justfile`; do not replace its recipes with hand-assembled Cargo/rustfmt commands or assume a native build validates the embedded target.
- Keep completion evidence with the checklist or in the task report: recipes/checks actually run, outcomes, limitations, and follow-up blockers. Supplemental diagnostics and dry runs do not count as completed project validation.

## Project validation commands

Run these from the repository root. The [Justfile](../Justfile) is authoritative; this table is a navigation aid, not a replacement for reading recipe bodies and dependencies. Discover recipes with `just --list`; inspect a workflow without running it with `just --dry-run <recipe>`.

Vesper is a `no_std` embedded project. Recipes coordinate the custom `aarch64-metta-none-eabi` target, `build-std`, board CPU/cfg flags, feature matrices, linker scripts, warning policy, and QEMU runner. **Use `just clippy`, not bare `cargo clippy`.** The same rule applies to build/test/format workflows. Do not copy private helper commands or override away their configuration to get a passing result.

| Command | Scope |
|---|---|
| `just build` | Build nucleus and kickstart and produce the kernel binary; defaults to RPi4/hardware |
| `just build rpi3 qemu` | Build the RPi3/QEMU kernel configuration without starting QEMU |
| `just fmt-check` | Workspace formatting check using the configured nightly toolchain |
| `just clippy` | RPi3/QEMU build prerequisite, embedded Clippy across the defined board/feature combinations, and capability host-test linting |
| `just clippy-pre-push` | Default features on RPi3 and RPi4 plus capability host-test linting; not the full embedded matrix |
| `just clippy-object-host` | Focused native lint check of the capability library and its opt-in ABI test harness |
| `just lint` | Formatting, full embedded Clippy workflow, and host-tool Clippy |
| `just test-device` | Device integration tests and doctests with the target configuration and QEMU runner |
| `just test-debug-console` | Debug-enabled nucleus handler regression tests under QEMU; included in `just test` |
| `just test-chainboot` | Chainboot tests with its own linker script and target runner |
| `just test-object-host` | Opt-in capability ABI integration tests on the native host (currently AArch64) |
| `just test-host` | Capability ABI tests, then native `chainofcommand` tests |
| `just test` | Device, chainboot, capability-host, and host-tool test workflows |
| `just pre-push` | Formatting, shortened Clippy, and tests; does not itself push anything |
| `just ci` | Cleanup, lint, build, and tests; do not invoke its cleanup as an incidental check |

Choose the appropriate scope and report it accurately. Missing tools, failed prerequisites, and timeouts are blockers, not reasons to fall back silently to a less representative native Cargo command. Keep long-running recipes time-bounded. Inspect side effects before using recipes: interactive/debug sessions, hardware flashing/ejection, tool installation, hook setup, and dependency updates are not routine validation.

When a needed focused test has no recipe, propose a small `Justfile` addition rather than inventing a parallel workflow. Explicitly approved ad hoc diagnostics may provide supplemental evidence, but never replace configured project checks.

## Phase 1 — Confirm contracts and support boundaries

Reference: [status](nucleus_capabilities.md#status-and-authority), [responsibilities](nucleus_capabilities.md#target-responsibilities), [decision register](nucleus_capabilities.md#decision-register).

- [ ] Review the consolidated reference with the maintainer; record amendments without reviving the retired `kernel/nucleus/design.md` as a second authority.
- [ ] Confirm the implementation-status matrix against module declarations and dispatch; identify supported, unsupported, and excluded draft operations in code/docs.
- [ ] Record prerequisite decisions for the next slice using D1–D9. Leave unrelated decisions explicitly open instead of blocking all progress or guessing their answers.
- [ ] Confirm the shared-address-space/protection direction (D1) before introducing domain/VSpace behavior that would fix a different architecture by accident.
- [ ] Confirm the capability-manager/revocation trust boundary (D2) before enabling general derivation or reclamation.
- [ ] Agree the ordinary-operation schema template: operation ID, arguments/units, slot scope, authority, results, blocking, ownership, and failure/partial completion.
- [ ] Keep research examples and unimplemented operations clearly labeled; remove conflicting authoritative-looking diagrams/comments as the corresponding code is reconciled.

**Exit:** the next slice has an explicit contract, known support boundary, and no unresolved prerequisite architectural choice. D1–D9 need not all be closed at once.

## Phase 2 — One shared, testable ABI

Reference: [type numbering](nucleus_capabilities.md#object-type-numbering), [wire contracts](nucleus_capabilities.md#invocation-and-wire-contracts). Prerequisite: Phase 1 scope; D9 where schemas change.

- [ ] Separate shared ABI definitions from client/syscall dependencies so they can be tested without booting a kernel. Decide module/feature separation before adding a new crate.
- [x] Add `just test-object-host` for the opt-in capability host tests and include it in `just test-host` / `just test`. Include `just clippy-object-host` in the full and shortened Clippy workflows.
- [x] Reconcile all core constants/conversions with **CoreType**: Null `0`, Untyped `1`, Domain `2`, KeyTable `3`, Time `4`, Endpoint `5`, Notification `6`, EventCount `7`, Buffer `8`, Reply `9`, DebugConsole `127`.
- [x] Keep architecture kinds distinguished by `0x80`; distinguish category-local indices from complete wire IDs in conversions and error details.
- [x] Establish one canonical type declaration and exhaustive checks for every related representation. Coordinate kernel/client migration; do not preserve the contradictory old `ObjectType` numbering.
- [ ] Add full-width checked operation/slot/size decoding; reject high-bit aliases, invalid flag bits, out-of-range values, and arithmetic overflow before narrowing.
- [ ] Implement shared error encoding/decoding, preserving existing status meanings and unknown future errors/details. Eliminate competing per-family wire error spaces as each family migrates.
- [ ] Define shared fixed-width records and constants; require layout/offset/size assertions for user-visible memory structures. Leave kernel `KeyEntry` layout private.
- [ ] Record operation schemas and reserved IDs for the first supported slice; define unused/reserved argument treatment.
- [ ] Set the compatibility/support-discovery policy needed for current consumers (D9); document any coordinated rebuild requirement.
- [ ] Add ABI tests for type/error round trips, every known operation decoder, reserved values, high-bit inputs, rights masks, and record layouts.
  - [x] Cover all object-kind aliases, all 256 wire values and local-index inputs, wrong categories/reserved IDs, const constructors, one-byte layouts, and type-related error payloads with independent literal ABI expectations.

### Catalogue slice validation

The private catalogue macro now generates enums, aliases, checked local-index decoding, and typed-to-wire conversions. Existing public names remain; Time/Endpoint/Notification/EventCount wire values now follow the canonical IDs. No object handlers were enabled. The host test feature is opt-in so the standard harness is skipped by the existing freestanding test workflow; full ABI/client dependency separation remains unchecked.

Validation uses the repository recipes from an AArch64 macOS host:

| Recipe | Result and scope |
|---|---|
| `just fmt-check` | Passed workspace formatting |
| `just clippy` | Passed the RPi3/QEMU nucleus + kickstart build, all seven embedded board/feature configurations, and capability host-test linting |
| `just clippy-object-host` | Passed the focused capability host-harness lint check |
| `just test-host` | Passed all 10 capability ABI tests; the host-tool harness completed with zero test cases |
| `just test` | Passed device integration tests under QEMU, device doctest workflow, chainboot test recipe, and the host stage including all 10 capability ABI tests |

The restored device harness uses current crate/API paths and explicit startup/panic dependencies. Its shared test startup enters EL1 from the boot helper's EL2 context before executing kernel-mode tests; production boot is unchanged. GPIO/MMIO and mailbox-format tests retain their assertions, with the mailbox-format test using local storage rather than requiring kernel DMA mappings.

Coverage limits: the chainboot test recipe currently has no runnable test executable, the host tool has zero test cases, and passing the existing suite does not complete the later capability lifecycle/IPC work. Nonblocking compiler-cache access and toolchain/dependency future-compatibility warnings remain; they do not change recipe configuration or exit status.

- [x] Validate the catalogue changes through `just fmt-check` and `just clippy`, including the configured build prerequisite and full embedded feature matrix.
- [x] Run the capability host tests through `just test-object-host` (included in `just test-host` and `just test`) and the configured embedded test workflow; record the tested scope explicitly.

**Exit:** kernel and client share unambiguous checked wire definitions, with ABI-only tests independent of target assembly. Later families extend this core rather than inventing another protocol.

## Phase 3 — Repair the active syscall/console path

Reference: [wire contracts](nucleus_capabilities.md#invocation-and-wire-contracts), [authorization](nucleus_capabilities.md#authorization). Prerequisites: relevant Phase 2 definitions; console authority decision under D4.

- [x] Gate DebugConsole handler, bootstrap grant, userspace wrapper, and boot use behind opt-in `debug_kernel`; retain canonical IDs and the current debug mechanism, record deferred repairs beside `invoke`, and validate feature-off/on builds. This is the maintainer-approved debug-only availability slice, not completion of the safety work below.
- [ ] Validate exception class, SVC immediate, and permitted origin before dispatch; route non-SVC faults and user-copy recovery through the correct exception path.
- [ ] Replace panicking raw register conversions in nucleus entry with checked failures; specify the ordinary control-call register preservation/output contract with `libs/syscall`.
- [ ] Establish explicit caller/domain context; do not use absence of a current domain as an implicit grant to domain zero.
- [ ] Define the console operation's authority, byte/string/NUL behavior, maximum length or chunking policy, and pointer semantics.
- [ ] Introduce checked user-memory access for the console path, including caller-context authorization, range/length/overflow validation, input stability, and fault behavior. Do not relabel caller virtual addresses as trusted physical addresses.
- [x] Remove unnecessary pointer-derived mutable object access from the console path; do not make the repaired vertical slice depend on a known-unsound cast pending Phase 4.
- [ ] Bound console copying and terminator handling; return specified errors for malformed inputs rather than panicking.
- [ ] Decode and propagate console results in userspace; gate/remove unconditional semihosting diagnostics from ordinary wrapper behavior.
- [ ] Correct false-success/error-discarding behavior in already-included Domain/KeyTable wrappers, while leaving unimplemented kernel operations explicitly unsupported.
- [ ] Test console success, kernel error propagation, invalid/empty slots, excessive raw slot/op values, boundary lengths, invalid/unauthorized pointers, non-SVC faults, unsupported SVC immediates, and fault recovery without recursive capability dispatch.

### Debug-only availability slice validation

The maintainer-approved scope retains the current pointer-based mechanism and defers safety/ABI changes. `debug_kernel` is opt-in in nucleus, kickstart, and the client library; kickstart forwards it to the client. Kernel dispatch/object code, the bootstrap console grant, the client wrapper, and boot calls are gated. Type `127` and Write `0` remain defined with the feature off. `qemu`/`jtag` do not imply availability, and Cargo's release profile does not disable an explicitly requested debug kernel.

| Recipe | Result and scope |
|---|---|
| `just fmt-check` | Passed workspace formatting |
| `just test-object-host` | Passed 11 tests feature-off and 12 feature-on; includes unchanged catalogue IDs, operation decoding, and feature-enabled handle construction without executing SVC |
| `just clippy` | Passed after removing an unnecessary binding in the new host test; includes feature-off RPi3/QEMU nucleus + kickstart build, the original seven embedded configurations plus `debug_kernel` and `qemu,debug_kernel`, and both host feature states |
| `just build rpi3 qemu,debug_kernel` | Passed coordinated feature-enabled nucleus + kickstart release build |
| `just test-device` | Passed existing QEMU device integration tests and device doctest workflow, with `debug_kernel` off |

Coverage limits: these checks do not validate runtime console authorization, pointer safety, error propagation, exception recovery, or the feature-enabled boot demonstration. No console-specific runtime tests were added. Deferred changes and alternatives are documented beside `api::debug_console::invoke`; the other Phase 3 items remain unchecked. Nonblocking compiler-cache access and toolchain future-compatibility warnings remain. The next prerequisite for general console support is an approved caller/authority and buffer-access contract, not another implicitly enabled operation.

### Stateless console access slice validation

Removed both `as_object_mut::<DebugConsole>()` calls in active dispatch/handling. Core dispatch borrows the table entry read-only; the console handler accepts `&KeyEntry`, preserves type-mismatch checking/error precedence, decodes the existing operation, and calls the stateless writer without accessing the capability's object pointer. The debug gate, canonical IDs, pointer-based Write ABI, bootstrap grant, and client/transport behavior are unchanged. No new D1–D9 decision is implied.

`kernel/nucleus/tests/debug_console.rs` compiles the production API/object module trees. Its three QEMU cases cover wrong-kind/null entries (including inline regions), invalid operations before touching deliberately invalid write arguments, shared entry borrows with unchanged metadata, and empty/invalid table lookup before an installed entry reaches the handler. `just test-debug-console` runs this opt-in harness and is included in `just test`.

| Recipe | Result and scope |
|---|---|
| `just test-object-host` | Passed 11 feature-off and 12 feature-on ABI tests |
| `just test-debug-console` | Passed the new three-case QEMU harness after fixing its feature attribute and non-Debug error handling |
| `just fmt-check` | Passed workspace formatting |
| `just clippy` | Passed the configured build, all nine embedded configurations, and both host-test feature states |
| `just build rpi3 qemu,debug_kernel` | Passed coordinated debug-enabled nucleus + kickstart release build |
| `just test` | Passed device/doctest, chainboot, host, and new debug-console workflows; chainboot has no runnable test executable and the host tool has zero tests |

Coverage limits: handler/lookup rejection tests are not SVC entry/return or successful-output integration tests. No general rights checks, caller context, user-copy safety, raw-register validation, client error propagation, or guarded object storage were introduced. The new embedded test harness is compiled/run by its test recipe; the existing Clippy recipe does not explicitly lint that harness. General console support still requires the approved caller/authority and buffer-access contracts. Compiler-cache access and toolchain/dependency future-compatibility warnings remain nonblocking. Only the pointer-reference removal item is completed by this slice.

**Exit:** a real end-to-end operation demonstrates the standard decoding, authority, user-copy, and result pattern. Unsupported wrappers fail honestly.

## Phase 4 — Capability storage and domain lifetime

Reference: [identity](nucleus_capabilities.md#vocabulary-and-identity), [lifecycle](nucleus_capabilities.md#copy-move-deletion-and-revocation), [DCBs](nucleus_capabilities.md#domain-and-shared-dcb-contracts). Prerequisites: D2–D5 as applicable, plus D1 before protection-context bindings.

- [ ] Choose guarded object access, object/domain/slot reuse identity, synchronization, and owned-versus-borrowed handle semantics (D3).
- [ ] Move `KeyEntry` responsibility to kernel capability storage and adjust imports without changing unrelated APIs. Keep shared `ObjectType` in the ABI layer.
- [ ] Replace lifetime-erasing safe constructors and unrestricted pointer-to-reference casts. Establish unique kernel kind mappings and exclusive access through an owning context.
- [ ] Define pool backing/alignment/lifetime requirements, capacity behavior, zero-sized-type policy, object retirement, and reuse validation.
- [ ] Enforce KeyTable occupancy invariants: no counted null inserts; no entry mutation bypassing membership bookkeeping; checked bounds; failed operations preserve state.
- [ ] Finalize per-kind copy/move/delete rules, rights attenuation, destination authority, badges, bootstrap slots, and Domain.Grant semantics (D2–D4).
- [ ] Implement the minimal KeyTable lifecycle with same-table/same-object alias handling and atomic source/destination updates. Keep unresolved Revoke/derivation behavior unsupported.
- [ ] Allocate and retire private Domain state, DCB identity, KeyTable owner, and scheduler/protection relationships coherently.
- [ ] Define execution-context initialization and legal Activate/Suspend/Resume transitions, including blocked continuations and valid-budget requirements (D3/D7/D8). Implement only transitions supported by the substrate; keep the rest explicitly deferred until Phase 7.
- [ ] Reject invalid/released/stale domain IDs before indexing; retire in-flight references before domain reuse.
- [ ] Choose DCB size/stride/page capacity, visibility/discovery, publication/snapshot protocol, event-summary indexing, and mapped-record lifetime (D5).
- [ ] Replace duplicated/hardcoded DCB layout assumptions with shared constants and mandatory assertions; make userspace observations honor availability and reuse.
- [ ] Add model tests for pool/table exhaustion, null insertion, stale handles, same-object operands, move failure, rights attenuation, domain release/reuse, and DCB layout/publication.
- [ ] Run appropriate target checks for shared DCB access and kernel-private state isolation.

**Exit:** safe access no longer depends on type tags alone; domain identity and table membership have one enforced lifecycle. Features requiring unresolved revocation/protection decisions are not advertised as complete.

## Phase 5 — Memory and safe reclamation vertical slice

Reference: [memory contracts](nucleus_capabilities.md#resource-storage-and-memory-contracts). Prerequisites: storage/lifetime foundation, D1/D2/D4/D6.

- [ ] Decide Buffer's kernel-versus-userspace role while retaining its registered ID; settle mapping-context identity and ASID capability versus VSpace-binding ownership (D6).
- [ ] Define backing size/alignment separately from descriptor storage and slot/bookkeeping quotas. Account for pool allocation and resource provenance.
- [ ] Define retype request/destination schema and single-object versus batch semantics; reject kinds whose authority cannot originate from memory alone.
- [ ] Validate untyped size, absolute alignment, minimum watermark granularity, overflow, source/destination authority, device-memory restrictions, and destination vacancy.
- [ ] Implement transactional retype with explicit reservation/commit/rollback and per-kind initialization. Preserve allocation accounting on every pre-commit failure.
- [ ] Define and enforce initialization/sanitization before ordinary RAM is newly exposed across protection boundaries; distinguish intentional content-preserving sharing and device-memory policy.
- [ ] Implement frame/page-table backing and the selected translation/protection-context mapping path with architecture-validated layouts and permission ceilings.
- [ ] Track actual mapping identity, full supported virtual addresses, permissions/attributes, and teardown state; eliminate placeholder map/unmap success.
- [ ] Implement ASID allocation/binding and TLB-safe reuse where the target mapping model requires it. Keep unrelated I/O/IRQ operations explicitly unsupported until their contracts are implemented.
- [ ] Implement revocation/reclamation completion, including descendant/in-flight authority retirement, PTE removal, required TLB/device synchronization, and safe backing/metadata reuse.
- [ ] Add userspace mapping/buffer wrappers only after their unmap, sharing, aliasing, external-mutation, and revocation safety contracts can be enforced.
- [ ] Test tiny/oversized/misaligned regions, high virtual addresses, occupied destinations, pool exhaustion, rights denial, device restrictions, partial mapping failures, stale mappings, ASID reuse, and cross-domain reuse without stale-data disclosure.
- [ ] Run target integration for retype → mapping → access → unmapping/revocation → safe reuse, including failure paths.

**Exit:** the memory slice creates, uses, and retires resources without untracked mappings, overlapping allocations, leaked authority, or unsafe reuse.

## Phase 6 — Deferred completion, asynchronous primitives, and IPC

Reference: [communication](nucleus_capabilities.md#communication-and-deferred-completion). Prerequisites: domain/storage lifecycle; D4/D7/D9; memory slice if the IPC ABI uses a shared buffer.

### Completion foundation

- [ ] Choose message/register or IPC-buffer transport, output/clobber declarations, message capacity, status/error shape, and supported operation set (D7).
- [ ] Define open/closed wait identity, separate send and receive/reply timeouts, clock/units, no-wait/infinite encodings, cancellation, and late completion.
- [ ] Implement explicit completed/blocked/handoff outcomes with saved pending invocation state; schedule only after relevant borrows/guards have ended.
- [ ] Specify bounded wait/reply resource reservation and cancellation on domain/capability teardown.
- [ ] Define stable user-record decoding and retained-buffer/mapping lifetime across blocking; test adversarial mutation and unmap during pending operations.

### Notification and EventCount

- [ ] Finalize badge/caller-bit authority and notification waiter consumption versus broadcast; reconcile pending summaries with DCB indexing (D4/D5/D7).
- [ ] Define shared-payload memory ordering for signal/advance and observation, including already-satisfied waits, polling, and buffer-slot reuse; distinguish DMA/cache-coherency requirements (D7).
- [ ] Implement Notification Signal/Wait/Poll with checked results, authorization, and race-free wait registration, consumption, wakeup, and cancellation.
- [ ] Define EventCount overflow behavior and implement Advance/Await/Read with monotonic progress, independent readers, checked arithmetic, and race-free threshold wakeup.
- [ ] Add client wrappers using shared schemas/decoders; test invalid authority, no-pending poll, signal/wait races, multiple waiters, cancellation, counter overflow, reader lag, and shared-payload publication/reuse ordering.

### Endpoint and Reply

- [ ] Define Call/Send/Recv and Reply semantics together: payload words, badge, transfer counts/destinations, reply-slot reservation, rights, and receiver restrictions.
- [ ] Resolve nonblocking Send behavior and retire/reserve the conflicting Endpoint.Reply operation without silently reusing its number.
- [ ] Implement per-invocation queued payloads and one rendezvous path shared by sender-first and receiver-first arrivals.
- [ ] Implement one-shot reply creation, authorized delegation, successful consumption, cancellation, and caller/server teardown.
- [ ] Implement capability transfer with prevalidation/reservation and atomic commit; retain ownership on pre-commit failure in both kernel and client wrappers.
- [ ] Test all payload words, multiple distinct queued callers, both arrival orders, occupied transfer destinations, stale identities, failed replies, closed waits, timeouts, cancellation, and late replies.
- [ ] Run target tests that verify actual IPC registers/buffers and blocked-call resumption, not only object-level state transitions.
- [ ] Only after basic IPC passes, decide and implement ReplyRecv/Forward with explicit partial-completion and reply-authority transfer semantics, or leave them documented as deferred.

**Exit:** blocking does not lose wakeups or invocation state; reply/capability ownership remains defined on success, failure, cancellation, and teardown. Optional optimizations are either validated or explicitly deferred.

## Phase 7 — Time and userspace scheduling

Reference: [Time contracts](nucleus_capabilities.md#time-and-userspace-scheduling). Prerequisites: domain/DCB and completion foundations, D2/D4/D8/D9.

- [ ] Decide budget issuance/replenishment authority, donation as loan/transfer, unused-budget return, compatible merge conditions, and delete/yield/expiry semantics (D8).
- [ ] Choose wire/internal units, monotonic deadline clock, conversion/rounding/overflow behavior, and multicore budget ownership. Reconcile microsecond sketches with nanosecond DCB accounting explicitly.
- [ ] Separate Time-object storage allocation from issuance of positive CPU budget; maintain budget provenance and conservation.
- [ ] Implement Split/Merge/Query with explicit slots, checked results, transactional failures, and no double accounting.
- [ ] Implement donation, preemption, cancellation, and parent/donor resumption through the shared completion mechanism; account for IPC donation consistently.
- [ ] Complete Domain Activate/Suspend/Resume with initialized contexts, explicit authority, legal transitions, and blocked-completion/budget preservation; test suspend/resume while running, waiting, and out of budget.
- [ ] Align DCB consumed/remaining/activation/deadline observations with the scheduler contract without moving userspace scheduling policy into the kernel.
- [ ] Provide client ownership APIs that preserve budget on pre-commit errors and do not rely on unintended destructor side effects.
- [ ] Test conservation, insufficient budget, failed split/donation, incompatible merge, expiry, cancellation, nested delegation, and simultaneous spending attempts.
- [ ] Run target scheduling/accounting tests with a userspace policy example and verify return of unused budget according to the chosen contract.

**Exit:** storage cannot mint execution authority; delegated time is spent at most once; execution, observation, and ownership agree across layers.

## Final integration and reference maintenance

- [ ] Audit every exposed wrapper against actual dispatch and supported object transitions; no false success or undocumented stub behavior remains.
- [ ] Reconcile names consistently (`Key`, `KeySlot`, `KeyEntry`, `Time`, `EventCount`, and shared operation enums) without gratuitous public renames.
- [ ] Remove superseded draft code/comments only when their design intent is captured in the reference and no user work is lost. Update links/imports/status alongside removals.
- [ ] Check all public and shared-record documentation against implemented units, widths, IDs, ownership, failure, and blocking semantics.
- [ ] Run the accumulated ABI/model suite, appropriate target builds, and QEMU integration suite; record unavailable validation separately.
- [ ] Revisit open D1–D9 entries; mark decisions resolved only with their chosen contract/rationale, and keep deferred features visibly unsupported.
- [ ] Update the support matrix and checklist with validation evidence. Treat performance figures as measurements with target/workload context, not inherited research claims.
