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
| `just clippy` | RPi3/QEMU feature-off and debug-enabled build prerequisites, all nine embedded configurations, and capability host-test linting |
| `just clippy-pre-push` | Default features on RPi3 and RPi4 plus capability host-test linting; not the full embedded matrix |
| `just clippy-object-host` | Focused native lint check of the capability library and its opt-in ABI test harness |
| `just lint` | Formatting, full embedded Clippy workflow, and host-tool Clippy |
| `just test-device` | Device integration tests and doctests with the target configuration and QEMU runner |
| `just test-debug-console` | Debug-enabled nucleus handler and slot-identity regression tests under QEMU; included in `just test` |
| `just test-capability-boot` | Debug boot, actual issued-key handoff and real SVC success/error smoke test; in-guest assertions and QEMU exit status, included in `just test` |
| `just test-chainboot` | Chainboot tests with its own linker script and target runner |
| `just test-object-host` | Opt-in capability ABI integration tests on the native host (currently AArch64) |
| `just test-host` | Capability ABI tests, then native `chainofcommand` tests |
| `just test` | Device, chainboot, capability-host, host-tool, debug handler/storage and capability boot workflows |
| `just pre-push` | Formatting, shortened Clippy, and tests; does not itself push anything |
| `just ci` | Cleanup, lint, build, and tests; do not invoke its cleanup as an incidental check |

Choose the appropriate scope and report it accurately. Missing tools, failed prerequisites, and timeouts are blockers, not reasons to fall back silently to a less representative native Cargo command. Keep long-running recipes time-bounded. Inspect side effects before using recipes: interactive/debug sessions, hardware flashing/ejection, tool installation, hook setup, and dependency updates are not routine validation.

When a needed focused test has no recipe, propose a small `Justfile` addition rather than inventing a parallel workflow. Explicitly approved ad hoc diagnostics may provide supplemental evidence, but never replace configured project checks.

## Phase 1 — Confirm contracts and support boundaries

Reference: [status](nucleus_capabilities.md#status-and-authority), [responsibilities](nucleus_capabilities.md#target-responsibilities), [decision register](nucleus_capabilities.md#decision-register).

- [ ] Review the consolidated reference with the maintainer; record amendments without reviving the retired `kernel/nucleus/design.md` as a second authority.
- [ ] Confirm the implementation-status matrix against module declarations and dispatch; identify supported, unsupported, and excluded draft operations in code/docs.
- [ ] Record prerequisite decisions for the next slice using D1–D9. Leave unrelated decisions explicitly open instead of blocking all progress or guessing their answers.
- [ ] Resolve remaining D1 details under the selected architecture: hostile native Domains, capability-scoped trust, Domain = VSpace boundary, shared numerical meaning/cheap fbufs with protected-context fallback, no intra-Domain aliases, permitted cross-Domain sharing, and revocation over client borrows. Settle machine-local namespace/reservation conflicts, fault delivery, and per-target protection features; fbuf participant addresses must be agreed before mapping. Multi-node global address allocation is out of scope, and stronger stale-raw-pointer/temporal-VA prevention is far-future work delegated to outside mechanisms for now; do not make it a current prerequisite.
- [ ] Define adversarial agent/test work against confinement, including libOS bypass, malicious syscall inputs, alias-rule bypass, DMA programming bypass, and revocation/reuse races. Keep side-channel resistance explicitly deferred.
- [ ] Implement remaining D2 schemas under the selected authority split: direct management capability grants entail bookkeeping responsibility and composition-scoped TCB membership; multiple/hierarchical managers are policy, not a kernel-singleton or registration requirement.
- [ ] Agree the ordinary-operation schema template: operation ID, arguments/units, slot scope, authority, results, blocking, ownership, and failure/partial completion.
- [ ] Keep research examples and unimplemented operations clearly labeled; remove conflicting authoritative-looking diagrams/comments as the corresponding code is reconciled.

**Exit:** the next slice has an explicit contract, known support boundary, and no unresolved prerequisite architectural choice. D1–D9 need not all be closed at once.

## Phase 2 — One shared, testable ABI

Reference: [type numbering](nucleus_capabilities.md#object-type-numbering), [wire contracts](nucleus_capabilities.md#invocation-and-wire-contracts). Prerequisite: Phase 1 scope; D9 where schemas change.

- [ ] Separate shared ABI definitions from client/syscall dependencies so they can be tested without booting a kernel. Decide module/feature separation before adding a new crate.
- [x] Add `just test-object-host` for the opt-in capability host tests and include it in `just test-host` / `just test`. Include `just clippy-object-host` in the full and shortened Clippy workflows.
- [x] Reconcile all core constants/conversions with **CoreType**: Null `0`, Untyped `1`, Domain `2`, KeyTable `3`, Time `4`, Endpoint `5`, Notification `6`, EventCount `7`, Buffer `8`, Reply `9`, DebugConsole `127`. (2026-09-15: Buffer was subsequently removed from the catalogue entirely — Reply moved to the freed `8`, DebugConsole keeps `127`, IDs `9`–`126` reserved — when Buffer was selected as a userspace/libOS construct.)
- [x] Keep architecture kinds distinguished by `0x80`; distinguish category-local indices from complete wire IDs in conversions and error details.
- [x] Establish one canonical type declaration and exhaustive checks for every related representation. Coordinate kernel/client migration; do not preserve the contradictory old `ObjectType` numbering.
- [ ] Add full-width checked operation/slot/size decoding; reject high-bit aliases, invalid flag bits, out-of-range values, and arithmetic overflow before narrowing.
  - [x] Add checked KeyTable operation decoding from `u64` with a widening `u32` adapter; preserve IDs `0`, `1`, `2`, `4`, and reject reserved IDs/high-bit aliases without enabling the handler.
- [ ] Implement shared error encoding/decoding, preserving existing status meanings and unknown future errors/details. Eliminate competing per-family wire error spaces as each family migrates.
  - [x] Decode the existing status 1–25 baseline with checked detail widths and lossless unknown-response preservation; migrate Domain mutation wrappers. Other families and competing draft errors remain pending.
- [ ] Define shared fixed-width records and constants; require layout/offset/size assertions for user-visible memory structures. Leave kernel `KeyEntry` layout private.
- [ ] Record operation schemas and reserved IDs for the first supported slice; define unused/reserved argument treatment.
- [ ] Set the compatibility/support-discovery policy needed for current consumers (D9); document any coordinated rebuild requirement.
- [ ] Add ABI tests for type/error round trips, every known operation decoder, reserved values, high-bit inputs, rights masks, and record layouts.
  - [x] Cover all object-kind aliases, all 256 wire values and local-index inputs, wrong categories/reserved IDs, const constructors, one-byte layouts, and type-related error payloads with independent literal ABI expectations.

### KeyTable operation-decoder slice validation

Selected the smallest remaining KeyTable ABI prerequisite, not its lifecycle implementation. `KeyTableOp` now decodes the existing operation vocabulary through `TryFrom<u64>` and a delegating `TryFrom<u32>`, returning the existing `InvalidOperation` error for every other value. No wire IDs, client request/result encoding, authority rules, or object transitions changed. The kernel handler remains excluded; active dispatch still rejects KeyTable as unsupported. D1–D9 remain unchanged because no new operation schema or behavior is enabled.

Three tests in the existing host harness pin all four operation IDs and the one-byte enum layout, reject every unassigned byte including `3`, and reject aliases with each high bit from 8 through 63 plus maximum-width values. Existing wrapper tests continue checking literal request encoding and error propagation.

| Recipe | Result and scope |
|---|---|
| `just test-object-host` | Passed 30 feature-off / 31 feature-on tests |
| `just fmt-check` | Passed workspace formatting |
| `just clippy` | Passed the RPi3/QEMU nucleus + kickstart build, all nine embedded configurations, and both host-harness feature states |

Coverage limits: no QEMU runtime tests or full `just test` run for this pure decoder addition. The decoder is not wired into active KeyTable dispatch because the operations remain unsupported. Raw SVC entry still has panicking register conversions; this slice does not repair that boundary or complete full-width argument/slot/size decoding. General KeyTable activation still requires guarded storage/lifetime and approved D2–D4 authority/lifecycle semantics. Existing cache-access and toolchain future-compatibility warnings remain nonblocking. The parent decoding item remains unchecked.

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
  - [x] Reject absent caller identity in private-domain/DCB accessors and active capability dispatch; explicitly select the existing boot fixture's domain. Exception-origin/caller binding and coherent domain lifecycle remain pending.
- [ ] Define the console operation's authority, byte/string/NUL behavior, maximum length or chunking policy, and pointer semantics.
- [ ] Introduce checked user-memory access for the console path, including caller-context authorization, range/length/overflow validation, input stability, and fault behavior. Do not relabel caller virtual addresses as trusted physical addresses.
- [x] Remove unnecessary pointer-derived mutable object access from the console path; do not make the repaired vertical slice depend on a known-unsound cast pending Phase 4.
- [ ] Bound console copying and terminator handling; return specified errors for malformed inputs rather than panicking.
- [ ] Decode and propagate console results in userspace; gate/remove unconditional semihosting diagnostics from ordinary wrapper behavior.
- [ ] Correct false-success/error-discarding behavior in already-included Domain/KeyTable wrappers, while leaving unimplemented kernel operations explicitly unsupported.
  - [x] Preserve specific errors and unknown status/details in Domain Activate/Grant/Suspend/Resume without enabling the excluded handler.
  - [x] Preserve specific errors and unknown status/details in KeyTable `copy_derive`, `delete`, `revoke`, and `grant_to`, without enabling the excluded handler or changing request encoding.
  - [ ] Replace KeyTable's no-op `transfer` placeholder after its interface/ownership contract is settled; do not advertise Move support.
- [ ] Test console success, kernel error propagation, invalid/empty slots, excessive raw slot/op values, boundary lengths, invalid/unauthorized pointers, non-SVC faults, unsupported SVC immediates, and fault recovery without recursive capability dispatch.

### Absent-caller rejection slice validation

Removed the `unwrap_or(0)` fallback from both `Nucleus::current_domain_mut` and `current_dcb_mut`. Missing caller identity now returns `None`; active dispatch propagates the existing `InvalidDomain` status without consulting domain zero's table. The existing boot fixture explicitly selects its first domain after creation, preserving its debug invocation path without introducing a general bootstrap layout or activating Domain operations. Private-domain lookup still checks pool bounds/allocation. No shared ABI, transport, rights, or object layout changed.

Three regression cases extend `kernel/nucleus/tests/debug_console.rs` using the production module trees and valid test-owned backing. They cover absent/cleared caller identity despite an installed domain-zero console, the existing error's wire/client round trip, explicit domain-zero dispatch, a distinct caller's empty table, unallocated/out-of-range/released private-domain IDs, and absent caller identity despite installed DCB records. The six-case QEMU harness failed before the production fix and passed afterward; an initial test compile error from consuming `CapError::code()` twice was corrected first.

| Recipe | Result and scope |
|---|---|
| `just test-object-host` | Passed 30 feature-off / 31 feature-on ABI/client tests |
| `just test-debug-console` | Passed all six cases under QEMU after the fix |
| `just fmt-check` | Passed workspace formatting |
| `just clippy` | Passed configured RPi3/QEMU build, all nine embedded configurations, and both host-harness feature states |
| `just build rpi3 qemu,debug_kernel` | Passed coordinated debug-enabled nucleus + kickstart release build |
| `just test-device` | Passed existing QEMU device integration and doctest workflow |

Coverage limits: these tests call production dispatch/accessors, not real SVC entry/return or the boot demonstration. Caller-origin binding, raw-register validation, domain incarnation/reuse, coherent private-domain/DCB allocation and DCB publication remain unfinished; the parent caller-context item stays unchecked. The test fixture does not claim Untyped-backed production storage or mapping isolation. Full `just test` was not run, and the Clippy recipe does not explicitly lint this embedded harness. Existing compiler-cache access and toolchain future-compatibility warnings remain nonblocking.

Follow-up superseded by the approved key package: migrate directly to full-width incarnation-bearing keys and the agreed InvalidKey/InconsistentKey diagnostics, rather than introduce an intermediate malformed-slot ABI. Then finish checked entry/origin/caller binding before enabling the guarded KeyTable lifecycle.

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

### Domain client-result slice validation

Follow-up: centralized all existing wire status numbers in `libs/object/src/syscall_status.rs`. Both the error encoder and decoder use these symbols, and nucleus uses the shared `SUCCESS` constant. Numeric meanings and unknown-response handling are unchanged; a literal ABI test pins every constant independently. Validation: `just test-object-host` passed 24 feature-off / 25 feature-on tests; `just fmt-check` and full `just clippy` passed after correcting a documentation lint. Target/QEMU runtime tests were not rerun for this symbolic-only follow-up.

Selected the Domain portion of the included-wrapper repair: its four mutation wrappers now use the shared `decode_syscall_result`, preserving request encoding and returning specific kernel errors instead of collapsing every failure into `Unknown`. Kernel dispatch still returns `UnsupportedCoreType(Domain)` after successful lookup. No operation, rights policy, lifetime, DCB access, scheduling transition, or new wire schema was enabled; D1–D9 remain open as previously recorded.

The decoder preserves success words and existing statuses 1–25, checks details before narrowing, and retains unknown/malformed responses verbatim in the client-only `UnknownResponse` variant. Its nonzero status cannot re-encode as success. Downstream exhaustive Rust matches need the added variant; no coordinated wire migration is required for this unchanged encoding.

| Recipe | Result and scope |
|---|---|
| `just test-object-host` | Passed 23 feature-off / 24 feature-on tests, including nine result-decoder tests and three Domain wrapper tests |
| `just fmt-check` | Passed workspace formatting |
| `just clippy` | Passed the configured RPi3/QEMU build, all nine embedded feature configurations, and both host-test states after fixing test-module visibility/configuration |
| `just test-device` | Passed the existing QEMU device integration and device doctest workflow |

The host harness compiles the actual Domain source with a test-only mock transport; it checks all four methods, full-width slot encoding, success, unsupported/lookup/rights errors, unknown statuses, malformed details, and unchanged handles. It never executes host SVC or dereferences DCB mappings. These are not real SVC/Domain lifecycle integration tests, and the AArch64 host cannot exercise a narrower-`usize` overflow branch. Compiler-cache access and toolchain future-compatibility warnings remain nonblocking. Full `just test` was not run for this slice. The remaining KeyTable result repair is recorded below; enabling Domain behavior still requires the guarded storage, authority, execution-context, completion, and time prerequisites.

### KeyTable client-result slice validation

Selected the remaining syscall-backed KeyTable wrapper repair. `copy_derive`, `delete`, `revoke`, and `grant_to` now use the existing shared decoder; `grant_to` delegates to `copy_derive` with the same all-rights request. Nonzero statuses no longer become false success or collapse into `Unknown`. Signatures, operation IDs, slot encoding, and wire errors are unchanged. The kernel handler remains excluded and active dispatch still reports `UnsupportedCoreType(KeyTable)` after successful lookup. No authority, lifecycle, or D1–D9 decision was introduced.

The existing host harness compiles the production KeyTable source with a test-only transport. Three new tests cover all four methods: full-width slot and rights encoding, exactly one call, success, unsupported/lookup/occupied-slot/rights errors, unknown statuses, malformed details, and unchanged handles. Literal expectations pin the used operation IDs and request masks without approving their eventual authority semantics.

| Recipe | Result and scope |
|---|---|
| `just test-object-host` | Passed 27 feature-off / 28 feature-on tests after correcting the test's `Rights` construction |
| `just fmt-check` | Passed workspace formatting |
| `just clippy` | Passed configured RPi3/QEMU build, all nine embedded feature configurations, and both host-test states after correcting a documentation lint |
| `just test-device` | Passed existing QEMU device integration tests and device doctest workflow |

Coverage limits: mock responses test client propagation, not kernel authorization, real SVC return, or table mutation/rollback. Full `just test` was not run. Existing compiler-cache access and toolchain/dependency future-compatibility warnings remain nonblocking. The no-op `transfer`, missing public handle construction, unused legacy `revoke` table argument, and unresolved all-rights delegation policy remain deferred. The parent wrapper-cleanup item stays unchecked because the no-op still exists. Enabling actual KeyTable operations next requires guarded storage/lifetime and approved D2–D4 lifecycle/authority semantics; this slice does not start that work.

**Exit:** a real end-to-end operation demonstrates the standard decoding, authority, user-copy, and result pattern. Unsupported wrappers fail honestly.

## Phase 4 — Capability storage and domain lifetime

Reference: [identity](nucleus_capabilities.md#vocabulary-and-identity), [lifecycle](nucleus_capabilities.md#copy-move-deletion-and-revocation), [DCBs](nucleus_capabilities.md#domain-and-shared-dcb-contracts), and [lifetime/authority decisions and research](lifetime-and-authority.md). Prerequisites: D2–D5 as applicable, plus D1 before protection-context bindings.

Maintainer direction is partially resolved: keys name capability incarnations; Retype origins carry delegable lifetime-control permission; creators retain their own authority after delegation; SPeCK-like resource mechanisms support trusted KeyMaster derivation management and kernel object retirement; abandoned allocations may leak without kernel recovery. Correctness comes before representation optimization: change object/key fields and layouts as needed, updating all dependent calculations/views/tests; optimize in a later measured pass. Remaining schemas/mechanisms are still open, and no implementation is marked complete by these decisions. Excluded operation families remain intended functionality, not rejected designs.

Maintainer foundation decisions (2026-09-06): ordinary keys use implicit caller-table context, with initially 32-bit slot and 32-bit incarnation fields and separate shared-object identity, without freezing total key size permanently. Optional embedded type remains a proposal and would take 8 slot bits, not incarnation bits. Capacity is fixed for now through an easily changed constant; runtime resizing is outside initial scope. Move yields a destination-local key and invalidates the source on commit; generations must not silently wrap. All managed storage/metadata comes from accounted Untypeds, with retyped KeyTable backing made kernel-private. Enforce single-core execution and defer SMP; use short-lived guarded access and explicitly handle aliased operands even under the kernel lock. Table management follows ordinary source/destination capability permissions without manager-identity exceptions. Kickstart establishes initial Untypeds/KeyTables and hands authority onward. Follow-up choices: derive rather than mutate authority in place; exhaust only the slot when its 32-bit incarnation is consumed, retaining deletion; shared inconsistency error with diagnostic reasons; incarnation-checked management selectors; separate table-management permissions; vacant destinations and rejected same-slot Move; initial target-kind allowlist of KeyTable and debug-gated DebugConsole. Retype uses only an Untyped's unused watermark range with no outstanding access; it does not reclaim or unmap arbitrary live allocations. These decisions complete no implementation or validation checkbox.

Approved key package (2026-09-07): packed incarnation-high/slot-low 64-bit keys without a type tag; non-owning typed handles; increment on committed installation from 1, retained counters on deletion and slot-local exhaustion; InvalidKey/InconsistentKey/KeySlotExhausted statuses 26–28 with diagnostic reasons; CopyDerive register packing and destination-local results; no live-Domain logical table rebinding; independent 64-bit kernel allocation generations; explicit actual-key bootstrap handoff and coordinated rebuild without slot-only fallback. See the canonical contract for exact encodings. Concrete guarded metadata/backing, rights, bootstrap mechanism and domain integration remain work. Prototype code may be replaced rather than preserved as a compatibility constraint; pre-existing design-intent comments remain protected.

- [ ] Implement the approved key package across shared/client keys, entry/transport, slot storage, bootstrap and tests. Synchronize capacity/layout/accounting consumers; do not freeze total key size. Implement slot-local exhaustion with cleanup allowed and no in-place rights/badge mutation. Implement independent kernel allocation identities and no-reset/no-rebinding enforcement before advertising general lifetime safety.
  - [x] Implement packed RawKey/non-owning Key, included client/transport migration, active full-width key/op decoding, checked slot lookup/removal, commit-only incarnation advancement and exhaustion, shared errors 26–28, and actual-key debug bootstrap handoff; validate ABI, storage, dispatch and real SVC paths. General object/domain lifetime and reusable metadata remain pending.
- [x] Specify authoritative metadata/backing ownership and the concrete guarded access API; enforce single-core/non-reentry execution, resolve same-object operands without aliased mutable references, and end guards before scheduling. SMP is deferred. (Guarded-access slice, 2026-09-07: `ObjectId` pool/index/generation identities in `KeyEntry`, per-pool authoritative `SlotMeta` validated before dereference, `!Send` per-invocation `Access` context with borrow-checked guard lifetimes, alias-rejecting pair resolution. Untyped-backed pool ownership and kernel-private KeyTable backing remain Phase 5.)
- [ ] Assign shared inconsistency status/reason encodings and precedence; implement diagnostic rejection for stale slot incarnation, invalidated capability and retired object, without returning replacement keys or automatic refresh. Keep ordinary resource operations, including authorized Frame remapping, distinct from capability replacement.
- [ ] Implement authoritative shared-object generation checks across tables, adding/changing Frame/object/key metadata as needed rather than preserving current layouts; do not require ancestry traversal merely to detect object retirement.
- [ ] Audit allocation/accounting, backing alignment/size, pool capacity, table/DCB strides and page views, initialization bounds, shared ABI consumers, and layout tests with each representation change; defer optimization to a separate measured pass.
- [ ] Enforce explicit source/destination management authority for derivation/installation/transfer and define retirement-to-background-cleanup handoff to entrusted managers; do not require central post-hoc registration.
- [ ] Define bookkeeping/failure/coordination protocols for the chosen libOS composition, permitting multiple/hierarchical managers in its TCB; settle selective branch invalidation of a still-live shared object separately.
- [ ] Implement permission-based retirement and preserve creator control after object delegation; test accepted allocation leaks without automatic final-capability destruction or conflicting reuse.
- [ ] Move `KeyEntry` responsibility to kernel capability storage and adjust imports without changing unrelated APIs. Keep shared `ObjectType` in the ABI layer.
- [x] Replace lifetime-erasing safe constructors and unrestricted pointer-to-reference casts. Establish unique kernel kind mappings and exclusive access through an owning context. (2026-09-07: `ObjectRef` removed; `KeyEntry::as_object`/`as_object_mut` raw derefs replaced by `object_id()` + `Access` resolution; unique `PoolTag` per pooled `NucleusObject`.)
- [ ] Define Untyped-backed pool/metadata ownership, alignment/lifetime requirements, capacity behavior, zero-sized-type policy, object retirement, and reuse validation; establish kernel-private KeyTable backing through unused-watermark allocation with no outstanding access, not an ordinary-Retype withdrawal protocol. (2026-09-12: carve primitives `carve_region`/`carve_pool` and the boot Untyped allocation are established in Kickstart's `bootstrap.rs`; the boot Domain pool and a boot KeyTable pool are carved, and the boot Domain's table lives in that pool. Runtime kernel-private KeyTable backing plus full pool/metadata ownership semantics remain. 2026-09-14: the KeyTable pool is removed — the boot table is carved and initialized kernel-privately by `build_initial_nucleus`, and runtime `Untyped.Retype` carves KeyTables from an Untyped's unused watermark range. Pool/metadata ownership for other kinds, retirement, reuse validation, and zero-sized policy remain.)
- [x] Enforce KeyTable occupancy invariants: no counted null inserts; no entry mutation bypassing membership bookkeeping; checked bounds; failed operations preserve state. Insertion failure also returns ownership of the submitted entry; this is internal storage, not enabled management syscalls.
- [x] Specify wire operands/results and rights bits/combinations for the approved CopyDerive/Move/Delete logical schemas: incarnation-checked source/target selectors, separately grantable table permissions, vacant destinations, rejected same-slot Move, preserved badges, CopyDerive attenuation and Move state preservation. Initial target kinds are KeyTable and debug-gated DebugConsole; leave other kinds/rebadging unsupported and settle Domain.Grant support/deferment (D2–D4). (2026-09-07: rights bits DERIVE/REMOVE/INSTALL, Move = DERIVE+REMOVE+INSTALL, wire schemas for all three ops, Domain.Grant deferred; see the management-schema slice validation below.)
- [ ] Specify Kickstart's predefined Domains, initial table capacities/slots/grants and incarnation-bearing handoff; account for boot/reserved memory without conflicting Untyped allocations. Numeric key passing alone does not install receiver authority. (2026-09-12: the inert-nucleus handoff is implemented — Kickstart carves the initial `Nucleus`, creates the boot Domain and its pooled KeyTable, installs boot-Untyped/debug-console grants, and records the anchor via `nucleus_set_anchor`; the exact predefined-Domain list, capacities, and incarnation-bearing handoff records remain. 2026-09-14: the boot KeyTable is carved and initialized kernel-privately rather than pooled, and a self-table capability is installed at `CAPTBL_SELF`.)
- [~] Implement the approved KeyTable/debug-console lifecycle with same-table/same-object alias handling, atomic source/destination updates and destination-local result keys. Permit authorized deletion of entries naming retired objects when slot incarnation still matches, without automatic object retirement. Keep unresolved Revoke/derivation, other target kinds, mapping teardown and Reply/Time-specific behavior unsupported. (Partial 2026-09-07: CopyDerive/Move/Delete handler active for the caller's own table via CAPTBL_SELF with atomic validate-commit and Move rollback. 2026-09-12: KeyTables are now pooled objects and the handler resolves source/destination tables through the guarded `Access` context, enabling cross-table CopyDerive/Move with alias-safe pair resolution and cross-table Move rollback; retired-object deletion cleanup still awaits the retirement transition. 2026-09-14: KeyTables are Retype-created carved objects referenced by address rather than pooled identities; cross-table resolution and rollback operate on carved tables through `resolve_carved` guards, and the boot test exercises cross-table CopyDerive after a runtime Retype.)
- [ ] Allocate and retire private Domain state, DCB identity, KeyTable owner, and backend protection relationships coherently: each Domain is an independent VSpace/protection boundary, not a member of a shared unprotected context. Reconcile separate ABI kinds/identifiers deliberately.
- [ ] Define execution-context initialization and legal Activate/Suspend/Resume transitions, including blocked continuations and valid-budget requirements (D3/D7/D8). Implement only transitions supported by the substrate; keep the rest explicitly deferred until Phase 7.
- [ ] Reject invalid/released/stale domain IDs before indexing; retire in-flight references before domain reuse.
- [ ] Choose DCB size/stride/page capacity, visibility/discovery, publication/snapshot protocol, event-summary indexing, and record reuse/availability within the intended persistent DcbView (D5).
- [ ] Replace duplicated/hardcoded DCB layout assumptions with shared constants and mandatory assertions; make userspace observations honor availability and reuse.
- [ ] Add model tests for pool/table/slot-incarnation exhaustion (including cleanup), null insertion, stale invocation/management selectors, inconsistency reasons, same-object operands, same-slot Move rejection, occupied destinations, atomic failure, retired-object entry deletion, separate permissions, attenuation/badge preservation, initial kind allowlist, domain release/reuse and DCB layout/publication.
- [ ] Run appropriate target checks for shared DCB access and kernel-private state isolation.

### Key wire and slot-identity slice validation (2026-09-07)

Implemented the approved RawKey encoding and non-owning typed handles, statuses 26–28 with lossless unknown-response decoding, included-client/transport migration, and full-width active key/op decoding. Table storage retains per-slot incarnation counters, rejects stale/deleted selectors, advances only on successful installation, allows final-incarnation deletion, and preserves entry ownership/accounting on insertion failure. Unrestricted mutable entry access is removed. CopyDerive clients encode the approved operands and return destination-local keys, but the management handler remains excluded; `transfer` returns explicit unsupported status. Console clients now propagate errors. Public Domain construction remains deferred to avoid exposing unfinished DCB observation safety.

The private debug-gated one-shot EL1 boot bridge returns the actual console installation result. Kickstart extracts its retained executable symbol from the paired nucleus image; no fake initialization SVC, guessed incarnation, or runtime key-refresh API remains. The boot Domain pool now uses aligned `MaybeUninit<Domain>` backing in the reserved nucleus BSS, sized from Domain rather than the old `0x1000`/16-KiB fixture. The loader accounts the complete BSS and mapping extent. General Untyped-backed metadata/pool ownership is not completed by this fixture.

| Recipe | Observed result |
|---|---|
| `just test-object-host` | Passed feature-off/on ABI and client tests, including new key/error round trips and packed wrapper requests |
| `just test-debug-console` | Passed 17 QEMU cases: nine handler/dispatch/caller cases and eight slot-storage cases |
| `just build rpi3 qemu,debug_kernel` | Passed coordinated debug build and boot-symbol extraction |
| `just test-capability-boot` | Passed actual issued-key handoff, successful Write through SVC and exact zero-incarnation InvalidKey response |
| `just clippy-object-host` | Passed both host harness configurations |
| `just clippy` | Passed both required paired-image builds, all nine embedded configurations and host harness linting |
| `just fmt-check` | Passed workspace formatting |
| `just test` | Passed complete configured workflow, including the new bounded capability boot smoke test |

Initial integration failures (CapError formatting and Clippy diagnostics) were fixed before the passing runs. Existing cache-access and Rust future-compatibility warnings remain nonblocking. No new nightly feature gates were added. Chainboot still has no runnable test executable and the host tool has zero test cases; the embedded regression harness is run by its recipe but not explicitly linted by Clippy.

Coverage limits: ObjectRetired is encoded/decoded but authoritative shared-object generation checks are not implemented. General object/domain retirement, no-reset/rebinding enforcement for reusable storage, rights/management transitions, exception-origin classification, hostile-EL0 confinement, user-copy safety, DCB publication and revocation remain unfinished. The boot smoke test intentionally stops after its success marker; it does not validate subsequent scheduling. The larger parent identity/lifecycle items remain unchecked. Next prerequisite is concrete authoritative object metadata/backing and the guarded access API, followed by the remaining management rights/operation schemas—not another slot-only ABI repair.

### KeyTable management-schema slice validation (2026-09-07)

Implemented the approved CopyDerive/Move/Delete wire schemas and table-permission rights matrix. Shared ABI gained `DERIVE`/`REMOVE`/`INSTALL` KeyTable-permission bits (per-kind interpretation, no renumbering) plus `Rights::has`/`permits`; clients gained a real `transfer` (Move) wrapper and schema-aligned `delete`, with `revoke` still unsupported. The kernel `api::key_table` handler decodes all three ops, checks table rights (CopyDerive = source DERIVE + destination INSTALL; Move = source DERIVE+REMOVE + destination INSTALL; Delete = REMOVE), enforces no-amplification, preserves badges on CopyDerive and full entry state on Move, rejects same-slot Move and occupied destinations, and rolls back the source on post-removal installation failure. Only the caller's own table (CAPTBL_SELF) is addressable; other KeyTable capabilities are rejected as unsupported rather than misresolved, pending pooled KeyTables. Revoke returns InvalidOperation.

| Recipe | Observed result |
|---|---|
| `just test-object-host` | Passed feature-off/on ABI and client tests (42 + 43), including Move request encoding and second-success-word handling |
| `just test-key-table` | Passed 9 QEMU cases: attenuation/badge preservation, amplification rejection, source/destination rights matrix, occupied destination, non-allowlisted kind, Move state preservation + source invalidation, same-slot Move rejection, Delete without retirement + stale-selector protection, Revoke/cross-table rejection |
| `just test-device` | Passed with the new test compiled under the default feature set (fixtures use the always-allowlisted KeyTable kind) |
| `just clippy` | Passed full embedded matrix and host harness |
| `just fmt-check` | Passed workspace formatting |
| `just test` | Passed complete configured workflow including the new `test-key-table` recipe |

Coverage limits: cross-table CopyDerive/Move (distinct table objects) is rejected pending pooled-KeyTable `Access` resolution and alias-safe pair access; retired-object Delete cleanup has no retirement transition to test against yet; the Move rollback path re-inserts with a fresh incarnation (original source key stays invalidated) and is not separately asserted. Domain.Grant remains deferred. Next prerequisite is pooled KeyTables with cross-table resolution, then the Phase 5 memory vertical slice.

### Guarded access and object-identity slice validation (2026-09-07)

Implemented the D3 concrete guarded-access selection. `KeyEntry` now stores only a checked `ObjectId` (pool tag, index, generation) — no raw object pointer — and `ObjectRef` plus `KeyEntry::as_object`/`as_object_mut` lifetime-erasing derefs are removed. `ObjectPool` carries authoritative per-slot `SlotMeta` (Free/Live/Retired + retained generation, no wrap, exhaustion prohibits reuse) with validation always preceding address computation. The new `Access` context is `!Send`, constructed per invocation under the kernel lock, and hands out borrow-checked `Guard`/`GuardMut`; `resolve_pair_mut` rejects same-slot aliases before constructing references. The debug console (stateless singleton, null identity, type-validated only) and the domain-pool fixture were migrated; the excluded `api/arch/vspace.rs` sketch is not compiled and keeps its design-intent form.

| Recipe | Observed result |
|---|---|
| `just test-object-host` | Passed feature-off/on ABI and client tests (43 + 44 cases) |
| `just test-debug-console` | Passed 17 QEMU cases plus 5 new pool model tests (generation/reuse, stale identity, wrong-pool/out-of-bounds, pool + generation exhaustion, pair alias rejection) |
| `just build rpi3 qemu` and `just build rpi3 qemu,debug_kernel` | Passed both configurations |
| `just clippy` | Passed full embedded matrix and host harness |
| `just fmt-check` | Passed workspace formatting |
| `just test` | Passed complete configured workflow |

Coverage limits: `Retired` state and the `ObjectRetired` diagnostic are represented but no retirement transition sets them yet. Guards are constructed only in tests so far; syscall-entry adoption of `Access` for pooled objects awaits pooled object types beyond Domain. Untyped-backed pool ownership, kernel-private KeyTable backing, and safe pool-backing lifetime remain Phase 5. The domain-pool fixture still uses index-based `get_live`/`get_live_mut` bootstrap access rather than capability-carried identities. Next prerequisite is the CopyDerive/Move/Delete management schemas and rights, then the Phase 5 memory vertical slice.

**Exit:** safe access no longer depends on type tags alone; domain identity and table membership have one enforced lifecycle. Features requiring unresolved revocation/protection decisions are not advertised as complete.

### Inert-nucleus bootstrap handoff slice validation (2026-09-12)

Implemented the D4 bootstrap handoff: the nucleus is now inert at boot and performs no initialization. The nucleus object model is exposed as a lib (`kernel/nucleus/src/lib.rs`) so Kickstart (the one-time boot code) constructs the initial `Nucleus` in memory carved from a boot Untyped's unused watermark range (`kernel/kickstart/src/bootstrap.rs`), installs the boot Domain and initial grants (boot Untyped at `KeySlot::BOOT_UNTYPED`, debug console at `KeySlot::DEBUG_CONSOLE`), and records the carved address via the exported `nucleus_set_anchor` setter. The nucleus reads that anchor (`NUCLEUS`, an `AtomicUsize`) under a separate `KERNEL_LOCK` on every syscall. The old lazy `NUCLEUS` fixture, `BOOT_DOMAIN_STORAGE`, and the in-nucleus `nucleus_bootstrap_debug_console` ceremony are removed. `boot_info::alloc_region` gained a guard against underflow on free fragments smaller than the request.

| Recipe | Observed result |
|---|---|
| `just build rpi3 qemu` | Passed (nucleus lib linked into kickstart; `nucleus_set_anchor` symbol extracted) |
| `just test-capability-boot` | Passed actual boot: Kickstart carves the initial Nucleus, installs grants, sets the anchor; the debug console write succeeds through SVC and the zero-incarnation key is rejected |
| `just test` | Passed complete configured workflow |
| `just clippy` | Passed full embedded matrix and host harness |
| `just fmt-check` | Passed workspace formatting |

Coverage limits: only the boot Domain pool is carved; KeyTable pooling (kernel-private KeyTable backing via unused-watermark allocation) and the full Untyped-backed pool/metadata ownership semantics (retirement, reuse validation, zero-sized policy) remain Phase 5. The boot Untyped is a single fixed-size region; the exact predefined-Domain list, capacities, and incarnation-bearing handoff records are not written. `KERNEL_LOCK` is a separate static lock rather than a field of the carved Nucleus; the locking model is not revisited for SMP. Next prerequisite is pooled KeyTables with cross-table resolution, then the Phase 5 memory vertical slice.

### Pooled KeyTables and cross-table resolution slice validation (2026-09-12)

Implemented the D3/D4 pooled-KeyTable foundation and cross-table management resolution. `NucleusPools` now owns a `keytables: ObjectPool<KeyTable>`; `Domain` references its capability table by `ObjectId` (`keytable_id`) instead of embedding it inline; Kickstart carves a boot KeyTable pool and installs the boot Domain's table there. The `api::key_table` handler now resolves the invoked source and destination table capabilities through the guarded `Access` context from the caller's own table, enabling `CopyDerive`/`Move`/`Delete` against distinct pooled tables. Same-object operands use a single mutable guard; distinct objects use the alias-rejecting `resolve_pair_mut` (CopyDerive) or a two-phase remove-then-install with source rollback (Move). Syscall entry (`handle_cap_invoke`) constructs the `Access` context and resolves the caller's table from the pool for dispatch, DebugConsole, and KeyTable paths.

| Recipe | Observed result |
|---|---|
| `just test-key-table` | Passed 12 QEMU cases: the prior same-table CopyDerive/Move/Delete/Revoke matrix plus cross-table CopyDerive, cross-table Move, and cross-table Move rollback on an occupied destination |
| `just test-debug-console` | Passed the existing debug console/dispatch harness against the pooled caller table |
| `just test-object-host` | Passed 42 feature-off / 43 feature-on ABI/client tests (unchanged) |
| `just test-device` | Passed existing QEMU device integration and doctest workflow |
| `just test-capability-boot` | Passed actual boot: Kickstart carves the KeyTable pool, installs the boot Domain's table, and the console write succeeds through SVC |
| `just build rpi3 qemu` and `just build rpi3 qemu,debug_kernel` | Passed both configurations |
| `just clippy` | Passed full embedded matrix and host harness |
| `just fmt-check` | Passed workspace formatting |
| `just test` | Passed complete configured workflow |

Coverage limits: retired-object Delete cleanup still awaits the retirement transition (no retirement path sets `Retired` yet). The boot KeyTable pool is carved at boot, not created by runtime Retype; kernel-private KeyTable backing via unused-watermark allocation and the full Untyped-backed pool/metadata ownership semantics remain Phase 5. The caller's own table is resolved through the pool on every invocation; no per-table caching or cross-pool alias set is added. `KERNEL_LOCK` remains a separate static lock; SMP is not revisited. Next prerequisite is the Phase 5 memory vertical slice.

## Phase 5 — Memory and safe reclamation vertical slice

Reference: [memory contracts](nucleus_capabilities.md#resource-storage-and-memory-contracts). Prerequisites: storage/lifetime foundation, D1/D2/D4/D6.

- [x] Decide Buffer's kernel-versus-userspace role while retaining its registered ID; settle mapping-context identity and ASID capability versus VSpace-binding ownership (D6). (2026-09-15: Buffer role selected — Buffer is a userspace/libOS construct over frame capabilities; no kernel Buffer object, handler, or pool. Follow-up the same day: the maintainer removed the Buffer kind from the core catalogue entirely rather than keeping the ID reserved — Reply moved to the freed 8, DebugConsole keeps 127, IDs 9–126 reserved. Mapping-context identity selected — the Domain is the mapping context, no separately targetable VSpace object, registered VSpace ID stays reserved. ASID model selected — explicit capability-protected ASIDPool/ASID resources with authorized AssignASID-style binding to the Domain's translation root. The excluded kernel Buffer sketches were removed and the userspace wrapper sketch moved under `userspace/buffer/` in the same slice.)
- [ ] Define backing size/alignment separately from descriptor storage and slot/bookkeeping quotas. Account for pool allocation and resource provenance.
- [x] Define retype request/destination schema and single-object versus batch semantics; reject kinds whose authority cannot originate from memory alone. (2026-09-14: wire schema selected and recorded in the contract; batch Retype implemented all-or-nothing; every non-allowlisted kind — including all kinds whose authority cannot originate from memory alone — is rejected with `InvalidObjectType`. 2026-09-15: `Frame` joined the allowlist — architecture-validated `size_bits` (`InvalidFrameSize` otherwise), carve aligned to the frame size, inline region capability.)
- [ ] Validate untyped size, absolute alignment, minimum watermark granularity, overflow, source/destination authority, device-memory restrictions, and destination vacancy. (2026-09-14: implemented for the KeyTable allowlist — size fit with `InsufficientMemory`, watermark alignment to the object alignment and the encoding granularity, checked overflow, `WRITE`/`INSTALL` authority, destination range/vacancy/capacity via `check_insert`. 2026-09-15: device sources are rejected in the general case — no creatable kind is device-capable — before any reservation, with rejection-path tests in the new `untyped` embedded test binary. Also 2026-09-15: absolute physical alignment — the carve address (base + watermark) is aligned, not just the watermark — and extent representability (unrepresentable `size_bits`/extents rejected with the region's own size as `InvalidSize`; the usable range ends at the watermark encoding bound) are implemented and tested. Also 2026-09-15: Frame sizing implemented — `size_bits` is arch-validated (12/21/30 on AArch64, `InvalidFrameSize` otherwise), the carve is aligned to the frame size, and frame extents reuse the general extent/watermark checks. Per-kind device policy remains.)
- [x] Implement transactional retype with explicit reservation/commit/rollback and per-kind initialization. Preserve allocation accounting on every pre-commit failure. (2026-09-14: validate → reserve → slot pre-validation → kernel-private initialize/install → watermark-last commit, with defensive rollback of installed capabilities; the watermark advances only on commit, so every pre-commit failure leaves accounting unchanged. Per-kind initialization is established with KeyTable as the initial allowlisted kind.)
- [x] Enforce Retype's selected allocation invariant: only an Untyped source, allocation from its unused watermark range, no overlapping prior allocation or outstanding access to the candidate bytes. Do not retype arbitrary live Frames/objects, rewind committed allocation state or add an unmap phase. Initialize KeyTable backing kernel-privately and publish only manipulation authority. Future reclamation must establish safe availability separately. (2026-09-14: enforced and verified, including consecutive-carve non-overlap assertions in the boot test.)
- [ ] Define and enforce initialization/sanitization before ordinary RAM is newly exposed across protection boundaries; distinguish intentional content-preserving sharing and device-memory policy. (2026-09-15: Frame-carve sanitization selected and implemented — the kernel zeroes the carved frame contents during the retype transaction, before capability installation and watermark commit; device sources are rejected earlier, so only ordinary RAM is zeroed. Intentional content-preserving sharing, device-memory policy, and other exposure paths remain open.)
- [x] Implement frame/page-table backing and the selected translation/protection-context mapping path with architecture-validated layouts and permission ceilings. (2026-09-15: `PageTable` joined the Retype allowlist — a sanitized 4 KiB carve whose capability is a checked pool identity over kernel metadata (carve address, walk level, installation record); tables are installed explicitly (`PageTable.Map` into a Domain root or a parent table) and `Frame.Map` walks the installed tables and writes real page/block descriptors with the permission ceiling (requested ⊆ frame rights, no execute, attrs zero = normal WB). See the mapping-foundation selection in the contract.)
- [x] Track actual mapping identity, full supported virtual addresses, permissions/attributes, and teardown state; eliminate placeholder map/unmap success. (2026-09-15: a frame capability records its single mapping as a dedicated payload — owning Domain identity plus the complete 48-bit virtual address, not a compressed address; `Frame.Unmap` clears the descriptor through that record; `PageTable.Unmap` requires an empty table and clears the parent reference; missing intermediates fail with `MissingIntermediate` (status 29) instead of fake success. The placeholder `set_mapped` bookkeeping is gone.)
- [x] Implement Copy as capability-only derivation with no active mapping association/PTE installation; define and implement separate Map with explicit target context, virtual address, permissions, and per-cap mapping bookkeeping. (2026-09-15: `Frame` joined the CopyDerive/Move/Delete allowlist — a derived frame starts unmapped while the original keeps its mapping; Move preserves the record; Delete leaves the mapping under accepted-leak. `Frame.Map` takes the explicit target Domain capability (bootstrap-era schema), virtual address, requested rights, and attributes, and records the per-cap mapping.)
- [ ] Specify/implement mapping-local Unmap versus origin-capability Revoke of descendants through KeyMaster/libOS; retain the origin unless separately unmapped/deleted, and define partial completion and prevention of racing mapping installation.
- [x] Implement the selected alias policy: no two virtual addresses for overlapping physical backing within one Domain, including through different caps/frame sizes; allow cross-Domain aliases and writable sharing. (2026-09-15: `Frame.Map` rejects a candidate whose physical extent overlaps any live descriptor in the target Domain with `PhysicalAlias` (status 30, detail 1 the conflicting physical base) ahead of the hardware transition; the arch-side interval walk covers derived caps and mixed frame sizes by construction and only walks the target's root, so cross-Domain aliases and writable sharing remain allowed. See the alias-policy enforcement slice below.)
- [ ] Implement fbuf setup that establishes addresses suitable for all participants before mapping; specify reservation/conflict handling and installation rollback before pointer publication. Keep machine-local allocation policy explicit and multi-node global address allocation out of scope; all shared pointees still need authorized mappings.
- [ ] Define per-target MMU/MPU/IOMMU, address-width/granularity, and DMA restrictions across the intended hardware range; support separate protected-context fallback without silently downgrading hostile-code isolation.
- [ ] Define encoding/enforcement of origin-only remap, virtual relocation versus physical replacement, descendant effects, and the interim full-revoke/no Frame-slot-reuse rule, including scope and exhaustion.
- [ ] Implement ASID allocation/binding and TLB-safe reuse where the target mapping model requires it. Keep unrelated I/O/IRQ operations explicitly unsupported until their contracts are implemented. (2026-09-15: allocation/binding and unmap invalidation implemented — boot-provided ASID pool with bitmap allocation (ASID 0 reserved for the kernel's boot context), `ASIDPool.Assign` binding with `GRANT`/`MAP` authority and failure atomicity, the bound ASID recorded on the Domain, and real ASID-scoped TLB invalidation on `Frame.Unmap` (by address) and root `PageTable.Unmap` (whole ASID) with dsb/isb completion. Also 2026-09-15: `Domain.Activate` installs the bound root into `TTBR0_EL1` with the bound ASID, making the tables hardware-live and the invalidation observable — the boot test reads through the live mapping, unmaps, remaps different backing at the same address, and verifies the stale translation was withdrawn. Remaining: ASID release on Domain teardown, hardware-safe ASID reuse, multi-pool partitioning of the 16-bit space, and deactivation/legal transitions with Domain scheduling.)
- [ ] Define trusted-libOS Unmap-before-invalidation prerequisites and the kernel checks/manager obligations preventing premature reuse; implement kernel retirement separately from KeyMaster background subtree cleanup.
- [ ] Implement the agreed revocation/reclamation completion protocol, including pending-use retirement, PTE removal, required TLB/device synchronization, and safe backing/metadata reuse; allocation leaks need not trigger kernel recovery.
- [ ] Implement MappedSlice ownership of a dedicated private capability/mapping with no derivations or independent management aliases; use incarnation-checked Drop cleanup and reject stale keys without touching replacements.
- [ ] Specify the preferred unsafe reference-access obligations and fbuf synchronization under authoritative revocation; client borrows must not veto revocation, and protection faults do not make invalid Rust references sound.
- [ ] Implement exclusive, immutable-shared, and mutable-shared modes with explicit transitions; distinguish a read-only mapping from backing with no other writers.
- [ ] Define revocation completion and fault delivery/likely Domain termination for withdrawn mappings; preserve hardware/TLB and physical-reuse safety. Preventing stale raw-pointer accesses after authorized VA reuse is delegated to outside mechanisms, not a current kernel implementation task.
- [ ] Test tiny/oversized/misaligned regions, high virtual addresses, occupied destinations, pool exhaustion, rights denial, device restrictions, partial mapping failures, stale mappings, ASID reuse, and cross-domain reuse without stale-data disclosure.
- [ ] Run target integration for retype → mapping → access → unmapping/revocation → safe reuse, including failure paths.

### Untyped Retype and carved KeyTables slice validation (2026-09-14)

Implemented the selected `Untyped.Retype` wire schema and the retype core. `nucleus::api::untyped` runs the transaction validate → reserve (extent representability, absolute carve-address alignment — base + watermark, not the watermark alone — and watermark-encoding-bounded fit) → destination-slot pre-validation (`KeyTable::check_insert` returns the same errors `insert` would: range, vacancy, incarnation capacity) → kernel-private object initialization and capability installation → watermark advance last, with defensive rollback of installed capabilities. Authority is `WRITE` on the invoked Untyped plus `INSTALL` on the destination table; the kind allowlist is `KeyTable` with `size_bits` zero, and every other kind is rejected. A device Untyped is rejected as a source for every creatable kind (per-kind device capability remains D6). KeyTables are no longer pooled: a `KeyTable` capability carries the carved object's kernel address in a per-kind `KeyTablePayload` (`KeyEntry::new_keytable`/`keytable_address`, `is_carved`), `Domain` references its table by address, and `Access::resolve_carved{,_mut,_pair_mut}` guard carved objects with same-address alias rejection; `KeyTable::advance_untyped_watermark` is the targeted commit mutation. Kickstart carves and initializes the boot table kernel-privately (`build_initial_nucleus`) and installs the self-table capability at `CAPTBL_SELF`. The userspace `UntypedKey::retype` wrapper encodes the schema through `protected_call6` and preserves kernel errors.

| Recipe | Observed result |
|---|---|
| `just test-object-host` | Passed 51 feature-off/on ABI/client tests, including the new Untyped operation-decoder and retype client wrapper tests |
| `just test-key-table` | Passed 27 QEMU cases: the management matrix against carved tables plus the storage/pool model tests |
| `just test-untyped` | Passed 22 QEMU cases: the included storage-model suite plus device-source rejection without state changes, rejection precedence over capacity validation and destination resolution, the RAM contrast reaching capacity validation, and unrepresentable size/extent plus watermark-encoding-bound rejections |
| `just test-debug-console` | Passed the console/dispatch harness against the carved caller table |
| `just test-capability-boot` | Passed actual boot: Retype through the real SVC path, cross-table CopyDerive into the new table, the three failure cases (invalid kind, occupied slot, insufficient memory), a second Retype plus post-carve derivation verifying consecutive-carve non-overlap, and a misaligned-base region carving an aligned, working table |
| `just clippy` | Passed the full embedded feature matrix and the host harness (includes the paired image builds) |
| `just fmt-check` | Passed workspace formatting |
| `just test` | Passed the complete configured workflow |

Coverage limits: per-kind device capability and device allocation policy remain open — all creatable kinds are rejected from device sources (D6). Sanitization before cross-boundary exposure, frames, mapping, ASID, revocation/reclamation, and retirement transitions remain later Phase 5 work; only the `KeyTable` kind is creatable. The defensive mid-run insert-failure rollback is pre-validated away rather than separately exercised. The exact predefined-Domain list, capacities, and incarnation-bearing handoff records remain Phase 4 items. Next prerequisite is the frame/page-table backing and mapping vertical slice.

### Frame Retype and carve sanitization slice validation (2026-09-15)

Extended the Retype allowlist with the `Frame` architecture kind (approved 2026-09-15). `nucleus::api::untyped` is now arch-typed (`invoke<A>`): `Frame`'s `size_bits` is validated by the target (`AArch64::validate_frame_size`: 12/21/30, `InvalidFrameSize` otherwise) before any source resolution; a frame is its own alignment (the absolute carve address is frame-aligned); the capability is inline (`KeyEntry::new_frame`) carrying the requested rights; the carve itself has no kernel object — instead the kernel sanitizes the frame by zeroing the carved contents before installing the capability and advancing the watermark (maintainer decision 2026-09-15). The transaction structure is unchanged: validate → reserve → destination pre-validation → initialize/install → watermark-last, with the defensive install rollback. Device Untypeds are rejected as frame sources like for every creatable kind. The embedded untyped test binary gained the frame rejection paths (nongranular sizes including 0, size rejection preceding source resolution, device frames, and the RAM contrast reaching capacity validation); the boot test carves 4 KiB and 2 MiB frames through the real SVC path, verifying the aligned carve addresses, full zeroing, exact watermark continuation (no gap/overlap), and the `InvalidFrameSize(13)` rejection.

| Recipe | Observed result |
|---|---|
| `just test-untyped` | Passed 26 QEMU cases: the prior 22 plus the four frame rejection paths |
| `just test-capability-boot` | Passed actual boot: 4 KiB and 2 MiB frame carves through the real SVC path with sanitization, alignment, non-overlap, and `InvalidFrameSize(13)` assertions |
| `just test-object-host` | Passed 52 feature-off and 53 debug-enabled ABI/client tests, including the new frame-kind encoding and `InvalidFrameSize` decode tests |
| `just test-key-table` / `just test-debug-console` | Passed 27 + 24 QEMU regression cases against the arch-typed dispatch |
| `just clippy` | Passed the full embedded feature matrix and the host harness (paired image builds included) |
| `just fmt-check` | Passed workspace formatting |
| `just test` | Passed the complete configured workflow |

Coverage limits: per-kind device capability and device allocation policy remain open (all creatable kinds still reject device sources). Sanitization beyond Retype-carved Frames — intentional content-preserving sharing, device memory, other exposure paths — remains D6. Frame mapping, ASID, revocation/reclamation, and retirement transitions remain later Phase 5 work; the 1 GiB granule (size_bits 30) is validated by the arch layer and rejection tests but not exercised as a successful boot carve. Next prerequisite remains the mapping vertical slice: Buffer's kernel-versus-userspace role, mapping-context identity, and ASID capability versus VSpace-binding ownership (D6), then frame/page-table mapping with real PTE installation.

### PageTable Retype and the mapping vertical slice validation (2026-09-15)

Implemented the selected mapping foundation (maintainer decisions 2026-09-15: explicit seL4-style table management, bootstrap-only explicit target `Domain` argument in `Frame.Map`, ASIDs deferred, boot root carved through the public Retype path). `PageTable` joined the Retype allowlist: a fixed architecture-validated 4 KiB carve (`InvalidSize` otherwise), sanitized (zeroed) like frames — stale descriptors would leak prior contents into hardware walks — with the capability as a checked pool identity over kernel metadata (`AArch64PageTable`: carve address, walk level, installation record `PtParent`). The metadata pool's backing is carved at bootstrap (`PoolCapacities.page_tables`, 16 slots) and its exhaustion is part of the Retype transaction: a batch that cannot fit releases its partially allocated slots before any carve write. `PageTable.Map` (op 0) installs a table into a parent named by capability type — a `Domain` capability with `MAP` installs the translation root (vaddr must be zero, root slot vacant), a `PageTable` capability installs one intermediate level (parent installed and below the leaf level, selected slot vacant, same-object alias rejected); `PageTable.Unmap` (op 1) requires an empty table and clears the parent reference (root unmap clears the Domain's `translation_root`). `Frame.Map` (op 0) resolves the target Domain capability, walks the installed tables, and writes the real page/block descriptor (4 KiB page at level 3, 2 MiB/1 GiB blocks at levels 2/1) with the permission ceiling (requested ⊆ frame rights, `READ` required, no execute — PXN|UXN, attrs zero = normal WB only) and vaddr validation (48-bit width, frame-aligned); `Frame.Unmap` (op 1) clears the descriptor through the frame's recorded mapping identity. A missing intermediate fails with the new `MissingIntermediate` status 29 (detail: faulting vaddr). The frame payload was restructured from the compressed `vaddr>>12` state to a dedicated `FramePayload` (paddr, full vaddr, owning Domain index+generation, size, flags) — 24 bytes inside the unchanged 32-byte slot. `Frame` joined the CopyDerive/Move/Delete allowlist with derived caps starting unmapped. The arch dispatch arms for `Frame` and `PageTable` are active; VSpace/ASID/I/O/IRQ handlers remain excluded sketches. Carved tables are not yet active in any hardware translation context, so unmap performs no TLB invalidation yet — this must become a real invalidation when Domain activation installs these tables into a TTBR. Userspace gained `FrameKey` (`map`/`unmap`/`get_extent`) and `PageTableKey` (`map`/`unmap`) wrappers preserving kernel errors.

| Recipe | Observed result |
|---|---|
| `just test-object-host` | Passed 61 feature-off and 62 debug-enabled ABI/client tests, including the new Frame/PageTable operation-decoder, `MissingIntermediate` round-trip, and wrapper wire-encoding tests |
| `just test-untyped` | Passed 29 QEMU cases: the prior 26 plus PageTable size rejection, device-source rejection for PageTable carves, and pool-exhaustion rollback releasing partially allocated metadata slots |
| `just test-capability-boot` | Passed actual boot: four PageTable carves through the real SVC path with sanitization, root installation into the boot Domain (via the new `SELF_DOMAIN` grant), the L1→L2→L3 chain, 4 KiB and 2 MiB frame mappings with hand-verified descriptor contents (valid/page-vs-block bits, AF, user RW/RO AP, PXN|UXN), double-map/missing-intermediate/misaligned/attrs rejections, derived-frame-unmapped, unmap clearing PTEs and records, empty-table-gated teardown of the whole chain, and pool-exhaustion rollback with a successful refill |
| `just test-key-table` | Passed 27 QEMU cases: the management matrix plus the Frame-derivation behavior in the allowlist test (Untyped still rejected, mapped frame derives unmapped, original keeps its mapping) |
| `just test-debug-console` | Passed the console/dispatch harness against the updated nucleus fixture |
| `just clippy` | Passed the full embedded feature matrix and the host harness (paired image builds included) |
| `just fmt-check` | Passed workspace formatting |
| `just test` | Passed the complete configured workflow |

Coverage limits: the no-two-VAs-per-Domain physical-overlap alias policy, fbuf setup, per-target MMU/IOMMU restrictions, origin-only remap (`Frame.Remap` returns a defined error), ASID allocation/binding and TLB invalidation, revocation/reclamation, and retirement transitions remain later Phase 5 work. The explicit target-Domain argument in `Frame.Map` is bootstrap-era mechanism until syscall caller identity exists. The 1 GiB granule is validated and rejected in tests but not exercised as a successful boot carve. Next prerequisite is the alias policy (physical-overlap enforcement within one Domain) or the ASID/Domain-activation slice that makes these tables hardware-live.

### Alias-policy enforcement slice validation (2026-09-15)

Implemented the selected alias policy — no two virtual addresses for overlapping physical backing within one Domain — ahead of the hardware transition. The arch layer gained `find_physical_overlap` (an `ArchObjects` associated function; the `AArch64` implementation walks every installed table from the target Domain's translation root, follows table descriptors recursively, and interval-checks each live block/page descriptor's physical extent against the candidate frame extent), and the `Frame.Map` handler rejects an overlapping candidate with the new `PhysicalAlias` status 30 (detail 1: the conflicting live mapping's physical base) before any descriptor write, so a rejection leaves every table and record unchanged. The check is physical, not capability-based — a derived capability naming an already-mapped frame is rejected at a second virtual address in the same Domain — and interval-based, so mixed frame sizes are covered by construction. It walks only the target Domain's root: cross-Domain aliases remain distinct PTEs and writable sharing is unaffected. The shared ABI gained `CapError::PhysicalAlias` (status 30; nonzero second detail words remain a lossless `UnknownResponse` fallback) and the userspace `FrameKey::map` wrapper preserves the kernel error.

| Recipe | Observed result |
|---|---|
| `just test-object-host` | Passed 63 feature-off and 64 debug-enabled ABI/client tests, including the new `PhysicalAlias` status pin (status 30, detail losslessness) and the frame wrapper error-preservation test |
| `just test-capability-boot` | Passed actual boot: the derived frame capability's second-vaddr map rejected through the real SVC path with `PhysicalAlias` naming the original frame's physical base; after unmapping the original, the derived cap mapped successfully at the second address (hand-verified PTE) and unmapped cleanly, proving the policy tracks live physical mappings rather than capability identity; the disjoint 2 MiB mapping coexists throughout |
| `just test-key-table` / `just test-untyped` / `just test-debug-console` | Passed 27 + 29 + 24 QEMU regression cases against the updated dispatch |
| `just clippy` | Passed the full embedded feature matrix and the host harness (paired image builds included) |
| `just fmt-check` | Passed workspace formatting |
| `just test` | Passed the complete configured workflow |

Coverage limits: only the boot Domain exists today (Domains are not Retype-creatable yet), so the cross-Domain alias success case is structurally guaranteed by the per-target-root walk but not exercised in boot; mixed-frame-size physical overlap is covered by the interval check by construction but not exercised, since Retype carves disjoint extents; overlap through device frames is unreachable because device sources reject Retype. Next prerequisite is the ASID/Domain-activation slice that installs these tables into a TTBR (making unmap's TLB invalidation real), or the mapping-local Unmap versus origin-capability Revoke specification.

### ASID allocation/binding and unmap TLB invalidation slice validation (2026-09-15)

Implemented the selected ASID model (maintainer decisions 2026-09-15: boot-provided pool, invalidation executed now rather than gated on activation — with the maintainer's remark that gating may be more efficient long-term once Domain activation exists). `AArch64ASIDPool` is real kernel state — a 512-slot allocation bitmap with ASID 0 reserved for the kernel's own boot translation context — allocated in a new boot-carved `asid_pools` pool (`PoolCapacities.asid_pools`, one slot). Kickstart provisions the boot pool kernel-privately and installs its capability at the new well-known `KeySlot::BOOT_ASID_POOL` (13; slots 5–12 are the boot test's Retype destinations). `ASIDPool.Assign` (op 0; `x2` target Domain key, `x3..x7` zero) is the active dispatch arm for `ArchType::ASIDPool`: `GRANT` on the pool capability plus `MAP` on the Domain capability, target root required (`NotMapped`), one ASID per Domain (`AlreadyMapped`), allocation last so every rejection is failure-atomic, success returning the ASID in `x1`. The `Domain` object records the bound ASID alongside its translation root. `Frame.Unmap` now issues the by-address invalidation (`tlbi vae1is` with the ASID in bits 63:48, then `dsb sy`/`isb`) through the new `ArchObjects::invalidate_tlb_by_vaddr`, and root `PageTable.Unmap` issues the whole-ASID invalidation (`tlbi aside1is`) through `invalidate_tlb_asid`; a Domain without a bound ASID has no hardware context, so no invalidation is issued. The superseded `invoke_asid_pool`/`invoke_asid` trait shims were removed (dispatch goes through the API handler like Frame/PageTable); the excluded VSpace sketch and the reserved `ASID` kind are untouched. Userspace gained `ASIDPoolKey`/`ASIDPoolOp` with the `assign` wrapper preserving kernel errors and rejecting out-of-range success words.

| Recipe | Observed result |
|---|---|
| `just test-object-host` | Passed 67 feature-off and 68 debug-enabled ABI/client tests, including the new ASIDPool operation-decoder, assign wire-encoding/success-decode, out-of-range rejection, and error-preservation tests |
| `just test-capability-boot` | Passed actual boot: `ASIDPool.Assign` through the real SVC path — pre-root `NotMapped`, non-Domain target `TypeMismatch`, successful bind of ASID 1 recorded on the boot Domain, double-assign `AlreadyMapped` — plus the existing unmap teardown now executing the real `tlbi vae1is`/`tlbi aside1is` sequences on the live kernel context without faulting |
| `just test-untyped` / `just test-key-table` / `just test-debug-console` | Passed 29 + 27 + 24 QEMU regression cases against the updated dispatch and fixtures |
| `just clippy` | Passed the full embedded feature matrix and the host harness (paired image builds included) |
| `just fmt-check` | Passed workspace formatting |
| `just test` | Passed the complete configured workflow |

Coverage limits: ASID release on Domain teardown, hardware-safe ASID reuse, and multi-pool partitioning of the 16-bit ASID space remain open (D6) — there is no release operation, so exhaustion of the 512-ASID boot pool is unreachable in tests. The invalidation is correct-by-construction but the tables are still not installed in any TTBR (Domain activation is future work), so no test observes a stale translation actually being withdrawn; the maintainer's long-term option of gating invalidation on live installation remains recorded. Rights denial on `Assign` is checked in the kernel but not exercised in boot, because `ASIDPool` is not on the CopyDerive allowlist (per-kind policy) so no attenuated pool capability can be built there; the host wrapper test covers the error decoding. Next prerequisite is the Domain-activation slice that installs a bound root into a TTBR (making the invalidation observable and starting Phase 7's legal-transition work), or the mapping-local Unmap versus origin-capability Revoke specification.

### Domain activation, execute right, and observable TLB invalidation slice validation (2026-09-15)

Implemented `Domain.Activate` (op 0, no arguments) as the translation-context installation step of activation (maintainer decision 2026-09-15), and — unblocked by the same decision — the frame `EXECUTE` right. `nucleus::api::domain` decodes op 0 only (Grant/Suspend/Resume return `InvalidOperation`), resolves the invoked Domain capability through the caller's table with `MAP` authority, requires a translation root and a bound ASID (`NotMapped` otherwise), restricts activation to the current Domain until scheduling exists (`InvalidOperation` otherwise), and commits through the new `ArchObjects::install_translation_context` — `TTBR0_EL1 ← root | asid << 63:48` with `dsb ish`/`isb` — idempotently. The `EXECUTE` right (bit 0x10, in `Rights::all()`) is requested through `Frame.Map`'s rights mask within the capability ceiling: with `WRITE` the arch layer installs kernel-privilege RW+X (AP=00 — EL1 cannot execute EL0-writable pages, the user-writable execute-never rule confirmed against QEMU's walk), without `WRITE` read-only executable at EL0 and EL1; without `EXECUTE` every mapping stays UXN|PXN. The bootstrap caller maps its own low-half image and stack (two fabricated 2 MiB frame fixtures, identity, RW+X) into the boot Domain's context before activating, so execution continues under the live tables — a real Domain's address space contains its own image and stack by construction. Two transport/entry bugs found and fixed en route: `protected_call0` now transmits zeros in all six argument registers (an unspecified register arrived as a defined-but-arbitrary argument, not "no argument" — with this fix the strict wire-argument convention selected the same day holds: unused words are zero on the wire and kernel-rejected otherwise, chosen over kernel-side ignoring as easier to validate and formalise), and the syscall entry distinguishes SVC from other synchronous exception classes (`ESR_EL1.EC` check) — previously a data/instruction abort was decoded as an invocation and re-executed forever with the "result" in x0..x2. Userspace gained `DomainKey::from_key` (public construction for the mutation path; the observation methods still require an established DCB mapping, D5) and the existing `activate` wrapper is now dispatched.

| Recipe | Observed result |
|---|---|
| `just test-capability-boot` | Passed actual boot: `Domain.Activate` through the real SVC path — pre-root and pre-ASID `NotMapped` rejections, then successful activation; the caller kept executing through the Domain's tables (kernel-privilege RW+X low-half blocks, hand-verified AP=00 and UXN|PXN clear); the live-context read returned the mapped frame's marker; `Frame.Unmap` withdrew the cached translation (the same-VA remap of different physical backing served the new contents, proving the invalidation); the boot identity context restored, the low-half blocks unmapped, and the existing teardown/pool-accounting assertions all passed |
| `just test-object-host` | Passed 68 feature-off and 69 debug-enabled ABI/client tests, including the new `DomainKey::from_key` construction test and the updated `grant_to` prototype-rights pin (0x1F with `EXECUTE`) |
| `just test-untyped` / `just test-key-table` / `just test-debug-console` | Passed 29 + 27 + 24 QEMU regression cases against the updated dispatch, rights, and arch layers |
| `just clippy` | Passed the full embedded feature matrix and the host harness (paired image builds included) |
| `just fmt-check` | Passed workspace formatting |
| `just test` | Passed the complete configured workflow |

Coverage limits: rights denial on `Activate` is checked in the kernel but not exercised in boot (the boot Domain cap carries `Rights::all()` and Domain is not on the CopyDerive allowlist); the non-current-Domain rejection is checked but unreachable in boot (only the boot Domain exists); ASID release/reuse, deactivation, and legal-transition enforcement remain open (D6/Phase 7); the `EXECUTE` right's user/privileged split is interim until EL0 entry exists. Next prerequisite is the mapping-local Unmap versus origin-capability Revoke specification, or Phase 7's full Activate/Suspend/Resume with execution contexts and budget.

**Deferred scope:** stronger temporal-VA quarantine/stale-pointer detection is far-future work, not an exit prerequisite. Revoked VAs need not remain inaccessible for a surviving Domain's lifetime. This does not defer kernel lifetime safety, capability incarnation checks, mapping withdrawal/TLB synchronization, or safe physical-resource reuse.

**Exit:** the memory slice creates, uses, and retires resources without untracked mappings, overlapping allocations, leaked authority, or unsafe physical reuse; it does not promise stale raw pointers always fault after authorized VA reuse.

## Phase 6 — Deferred completion, asynchronous primitives, and IPC

Reference: [communication](nucleus_capabilities.md#communication-and-deferred-completion). Prerequisites: domain/storage lifecycle; D4/D7/D9; memory slice if the IPC ABI uses a shared buffer.

### Completion foundation

- [x] Choose message/register or IPC-buffer transport, output/clobber declarations, message capacity, status/error shape, and supported operation set (D7). (2026-09-16: register transport with hybrid per-domain IPC-buffer spill; dedicated-word layout preserving the sketched label + five data words + one optional transfer slot; extended outputs `x1..x7` for IPC completions, ordinary two-word results unchanged; zero-or-one transferred capability; blocking operations return only on completion/cancellation, no blocked status; foundation + Notification/EventCount staged before Endpoint/Reply wide returns. Recorded in the contract's communication section and D7 register entry; implementation of the transport, buffer, and primitives remains open.)
- [ ] Define open/closed wait identity, separate send and receive/reply timeouts, clock/units, no-wait/infinite encodings, cancellation, and late completion.
- [ ] Apply the adopted rejected-before-admission / cancelled-before-commit / completed / outcome-unknown vocabulary to each local operation's commit guarantees; define wire representations separately. Keep remote/distributed protocols and policy out of the nucleus.
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
- [ ] Complete Domain Activate/Suspend/Resume with initialized contexts, explicit authority, legal transitions, and blocked-completion/budget preservation; test suspend/resume while running, waiting, and out of budget. (2026-09-15: the translation-context installation step of Activate is active — `Domain.Activate` installs the bound root into `TTBR0_EL1` with `MAP` authority, restricted to the current Domain; execution contexts, budget, DCB updates, Suspend/Resume, and legal-transition enforcement remain this item's work.)
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
