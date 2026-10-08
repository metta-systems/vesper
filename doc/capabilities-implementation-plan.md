# Capabilities implementation plan

The design authority is [capabilities contract](capabilities-contract.md). This checklist turns those contracts into dependency-ordered work across `libs/object`, `libs/syscall`, `kernel/nucleus/src/api`, `kernel/nucleus/src/objects`, and the nucleus entry/scheduler/backend code.

This is a TODO list, not a claim of implementation. This file holds only open work. A finished item leaves it as a one-line marker in [`capabilities-completed.md`](capabilities-completed.md); details live in the contract and history in version control. Mark an item `- [x]` only after its stated outcome is implemented/reviewed and the relevant validation has actually passed. Record blocked or unrun validation rather than checking it off.

## Working rules

- Select a small, explicitly scoped item or coherent group of items; do not execute the entire backlog merely because this file exists.
- Read the reference and relevant decision-register entries first. Resolve prerequisite decisions with the maintainer; do not silently promote a recommendation to a contract.
- Inspect module declarations, feature/target gates, call sites, and current local edits. Distinguish excluded sketches from reachable behavior.
- Change a contract here and in the reference before intentionally implementing a different ABI or semantic model.
- Complete the affected shared definitions, client encoding/decoding, kernel authorization, state transitions, and tests together. Unsupported operations must remain explicit errors.
- Preserve user work. This repository uses JJ: no raw Git, commits, history changes, new changes/branches, or pushes by default. Version-control mutation requires an explicit request.
- Use **`just` for project build, test, formatting, and lint workflows**. Read the current `Justfile`; do not replace its recipes with hand-assembled Cargo/rustfmt commands or assume a native build validates the embedded target.
- When an item is finished, replace it with a one-line marker in [`capabilities-completed.md`](capabilities-completed.md): what exists and where it is exercised. No narratives, dates or validation logs; version control keeps the history. Supplemental diagnostics and dry runs do not count as completed project validation.
- Keep [`doc/object_types/`](object_types/) current with every capability refactor: when a refactor invalidates a per-kind document, replace the no-longer-valid content with new data instead of layering amendments on top of stale text. The catalogue tables, wire IDs, operation lists, and status columns must describe the implemented reality after each slice. Documentation states the **current contract only** — no selection dates, no "selected/moved/renamed from" history, no "formerly…" chains; the contract and this plan carry the history.
- There is no previous version of anything: once a change is approved, preserve no backwards compatibility, migration path, deprecated alias, or legacy behavior. Functionality can be completely removed and replaced when necessary; update all consumers in the same slice.

## Project validation commands

Run these from the repository root. The [Justfile](../Justfile) is authoritative; this table is a navigation aid, not a replacement for reading recipe bodies and dependencies. Discover recipes with `just --list`; inspect a workflow without running it with `just --dry-run <recipe>`.

Vesper is a `no_std` embedded project. Recipes coordinate the custom `aarch64-metta-none-eabi` target, `build-std`, board CPU/cfg flags, feature matrices, linker scripts, warning policy, and QEMU runner. **Use `just clippy`, not bare `cargo clippy`.** The same rule applies to build/test/format workflows. Do not copy private helper commands or override away their configuration to get a passing result.

| Command | Scope |
|---|---|
| `just build` | Build nucleus and kickstart and produce the kernel binary; defaults to RPi4/hardware |
| `just build rpi3 qemu` | Build the RPi3/QEMU kernel configuration without starting QEMU |
| `just build-endpoint-test` | Build the endpoint-test e2e kernel image (nucleus + endpoint-test and its bundled components, sharing `libkicktest` scaffolding with the other test kernels); defaults to RPi3/QEMU |
| `just fmt-check` | Workspace formatting check using the configured nightly toolchain |
| `just clippy` | RPi3/QEMU feature-off and debug-enabled build prerequisites, all nine embedded configurations, and capability host-test linting |
| `just clippy-pre-push` | Default features on RPi3 and RPi4 plus capability host-test linting; not the full embedded matrix |
| `just clippy-object-host` | Focused native lint check of the capability library and its opt-in ABI test harness |
| `just lint` | Formatting, full embedded Clippy workflow, and host-tool Clippy |
| `just test-device` | Device integration tests and doctests with the target configuration and QEMU runner |
| `just test-debug-console` | Debug-enabled nucleus handler and slot-identity regression tests under QEMU; included in `just test` |
| `just test-capability`, `test-memory`, `test-sync`, `test-ppc` | Boot one e2e test kernel each (separate binaries reusing the real kickstart boot path, sharing `libkicktest`): capability handoff and Retype; translation tables, mappings, activation and AddressSpace retirement; Notification/EventCount and blocking through the Bounce fixture Thread; Invocation construction and PPC Call/Return into Bounce. In-guest assertions and the QEMU exit status are the result; all included in `just test` |
| `just test-endpoint` | Boot the endpoint-test e2e kernel: client, rendezvous-endpoint and server `AddressSpace`s meet through PPC `Invocation`s, blocking inside the endpoint; in-guest assertions and the QEMU exit status are the result; included in `just test` |
| `just test-fp-trap` | Boot fp-trap-test: an FP/SIMD instruction must trap at EL1t (test-only nucleus hook) and at EL0 (delivered to the component's fault handler); included in `just test` |
| `just test-fault` | Boot fault-test: EL0 fault delivery — skip, retry, terminate, Return faults, and every unhandled case parking the Thread as faulted; included in `just test` |
| `just audit-fp-simd` | Disassemble every image that runs under the integer-only policy and reject FP/SIMD instructions; part of `just lint` |
| `just test-chainboot` | Chainboot tests with its own linker script and target runner |
| `just test-object-host` | Opt-in capability ABI integration tests on the native host (currently AArch64) |
| `just test-host` | Capability ABI tests, then native `chainofcommand` tests |
| `just test` | Device, chainboot, capability-host, host-tool, debug handler/storage, and every e2e test kernel (capability, memory, sync, ppc, endpoint, fp-trap, fault) |
| `just pre-push` | Formatting, shortened Clippy, and tests; does not itself push anything |
| `just ci` | Cleanup, lint, build, and tests; do not invoke its cleanup as an incidental check |

Choose the appropriate scope and report it accurately. Missing tools, failed prerequisites, and timeouts are blockers, not reasons to fall back silently to a less representative native Cargo command. Keep long-running recipes time-bounded. Inspect side effects before using recipes: interactive/debug sessions, hardware flashing/ejection, tool installation, hook setup, and dependency updates are not routine validation.

When a needed focused test has no recipe, propose a small `Justfile` addition rather than inventing a parallel workflow. Explicitly approved ad hoc diagnostics may provide supplemental evidence, but never replace configured project checks.

## Remaining-work complexity

Relative engineering estimates including focused tests; not schedule commitments.

| Work | Complexity | Main cost |
|---|---|---|
| Retirement of the remaining kinds and scheduler-shared records | Large | Shared-memory publication and cross-subsystem identity |
| General selective revoke plus memory reclamation | Very large | Descendants, mappings, hardware, bounded completion |
| Safe revocable mapping abstractions / multicore Time | Very large | Protection and concurrency contracts |

## Phase 1 — Confirm contracts and support boundaries

Reference: [status](capabilities-contract.md#status-and-authority), [responsibilities](capabilities-contract.md#target-responsibilities), [decision register](capabilities-contract.md#decision-register). Open D1–D9 questions are in [`capabilities-decisions.md`](capabilities-decisions.md).

- [ ] Define adversarial agent/test work against confinement: libOS bypass, malicious syscall inputs, alias-rule bypass, DMA programming bypass, and revocation/reuse races. Side-channel resistance stays deferred.

## Phase 2 — One shared, testable ABI

Reference: [type numbering](capabilities-contract.md#object-type-numbering), [wire contracts](capabilities-contract.md#invocation-and-wire-contracts). Prerequisite: Phase 1 scope; D9 where schemas change.

**Phase complete:** every item has moved to [`capabilities-completed.md`](capabilities-completed.md#phase-2--one-shared-testable-abi). Version/support discovery for separately built components is parked in [`capabilities-decisions.md`](capabilities-decisions.md) under D9.

## Phase 3 — Repair the active syscall/console path

Reference: [wire contracts](capabilities-contract.md#invocation-and-wire-contracts), [authorization](capabilities-contract.md#authorization). Prerequisites: relevant Phase 2 definitions; console authority decision under D4.

**Parked** (maintainer decision): DebugConsole stays a debug-only prototype. Every remaining item below is console general-availability work. It waits on the console byte-transport decision parked in [`capabilities-decisions.md`](capabilities-decisions.md) under D1/D6/D9 and does not block later phases.

- [ ] Route user-copy fault recovery through the correct exception path, without recursive capability dispatch. Lands with the console's checked user-memory access below.
- [ ] Define the console operation's authority, byte/string/NUL behavior, maximum length or chunking policy, and pointer semantics.
- [ ] Introduce checked user-memory access for the console path, including caller-context authorization, range/length/overflow validation, input stability, and fault behavior. Do not relabel caller virtual addresses as trusted physical addresses.
- [ ] Bound console copying and terminator handling; return specified errors for malformed inputs rather than panicking.
- [ ] Test console success, kernel error propagation, invalid/empty slots, excessive raw slot/op values, boundary lengths, invalid/unauthorized pointers, non-SVC faults, and fault recovery without recursive capability dispatch.

## Phase 4 — Capability storage and Thread lifetime

Reference: [vocabulary and identity](capabilities-contract.md#vocabulary-and-identity), [authority, slots, and capability lifecycle](capabilities-contract.md#authority-slots-and-capability-lifecycle), [allocation and representation](capabilities-contract.md#allocation-and-representation), [Thread and AddressSpace contracts](capabilities-contract.md#thread-and-addressspace-contracts). Prerequisites: D2–D5 as applicable.

### Keys, identity and retirement

- [ ] Give carved objects (KeyTables, Frames) a retirement identity so retirement rejects old capabilities in every table without an ancestry walk.
- [ ] Implement permission-based retirement for the remaining kinds (only Thread and AddressSpace have `Retire`); test that creator control survives delegation and that accepted leaks cause no automatic destruction or conflicting reuse.
- [ ] Test KeyTable Delete of entries naming retired Threads and AddressSpaces (Delete accepts any kind and checks only the slot incarnation).
- [ ] Define the retirement-to-background-cleanup handoff to entrusted managers (no central post-hoc registration).
- [ ] Track the current Thread as an incarnation-checked identity, not a bare pool index (`Nucleus::current_thread`).
- [ ] Settle pool retirement, reuse validation and the zero-sized-type policy for `ObjectPool`; create pools at runtime, not only at bootstrap.
- [ ] After implementation experience, review the guarded key-space configuration (per-table guards, bare destination slots, guard-in-capability storage sourced from `SELF_KEYTABLE`) and confirm or revise it.

### Scheduler-shared records (D5)

- [ ] Remove `DcbPages`/`DcbView`, the global DCB view, `libs/object/src/domain.rs` and the duplicated `USER_BASE`/`MAX_DOMAINS` constants; replace them with scheduler-owned shared records behind shared constants and size/offset assertions.
- [ ] Implement `Scheduler.ShareRegion` across shared ABI, wrapper, dispatch and object state: one already scheduler-mapped Frame per call, reject conflicting re-share, check Scheduler and Frame authority.
- [ ] Implement the scheduler-declared fixed-stride record table: capacity, Thread identity/incarnation lookup, publication/snapshot protocol and record reuse. Kernel-private execution state stays private.
- [ ] Implement hierarchical Scheduler derivation, root-only Thread creation and Thread donation with strict tree structure; Kickstart establishes the root scheduler before scheduling starts.
- [ ] Move notification and block-reason accounting into the shared records, with event-summary indexing and Thread-retirement cleanup.
- [ ] Test scheduler/Frame authority, mapped and unmapped ShareRegion inputs, re-share rejection, stride placement, identity reuse, tree constraints, root creation/donation, publication/reuse and Thread teardown; run target checks for record access and kernel-private state isolation.
- [ ] Implement Brand once its binding is selected, in the IRQ slice.

## Phase 5 — Memory and safe reclamation

Reference: [memory contracts](capabilities-contract.md#resource-storage-and-memory-contracts). Prerequisites: D1/D2/D4/D6.

- [ ] Separate backing size/alignment from descriptor storage and slot quotas, with provenance, for kinds outside the kernel object pools.
- [ ] Define the device-memory policy per kind (today every device source is rejected except an Untyped split) and the sanitization duties beyond Retype-carved Frames and PageTables, distinguishing intentional content-preserving sharing.
- [ ] Implement mapping-local Unmap versus origin-capability Revoke of descendants: retain the origin, define partial completion, and prevent racing mapping installation.
- [ ] Encode and enforce origin-only remap, virtual relocation versus physical replacement, descendant effects, and the interim full-revoke/no-Frame-slot-reuse rule.
- [ ] Implement the revocation/reclamation completion protocol: teardown bookkeeping retained across partial map/unmap failures, Unmap before invalidation, pending-use retirement, PTE removal, TLB/device synchronization and safe backing/metadata reuse, with kernel retirement separate from manager subtree cleanup.
- [ ] Implement fbuf setup that agrees addresses for every participant before mapping, with reservation/conflict handling and rollback before pointers are published.
- [ ] Implement exclusive, immutable-shared and mutable-shared modes with explicit transitions, and `MappedSlice` ownership of a private, non-derivable capability and mapping with incarnation-checked Drop cleanup.
- [ ] Define per-target MMU/MPU/IOMMU, address-width and DMA restrictions, with a protected-context fallback that never downgrades hostile-code isolation.
- [ ] Implement `ASIDControl` pool creation and partitioning of the 16-bit ASID space once its schema is selected.
- [ ] Test rights denial for `Frame.Map`, `PageTable.Map` and `ASIDPool.Assign`; inject failures before and after Retype reservation and check ownership and accounting; test partial mapping failures, revocation races, cross-AddressSpace reuse without stale-data disclosure, and run target integration for retype → map → access → unmap/revoke → safe reuse, including failure paths.

## Phase 6 — Deferred completion and IPC

Reference: [communication](capabilities-contract.md#communication-and-deferred-completion). Prerequisites: D4/D7/D9.

- [ ] Apply the aborted-work vocabulary (rejected before admission / cancelled before commit / completed / outcome unknown) to each operation's commit point, and define its wire representation.
- [ ] Define stable user-record decoding and buffer/mapping lifetime across blocking; test adversarial mutation and unmap during pending operations.
- [ ] Test Notification/EventCount signal/wait races, multiple waiters, cancellation, reader lag and payload publication/reuse ordering with multi-Thread fixtures.
- [ ] Specify and implement Invocation distribution: CopyDerive restrictions, rights attenuation and badge-on-derive (D4), preserving target identity and the nonzero entry.
- [ ] Specify nested/concurrent-call rules beyond the fixed depth, shared-memory rules, optional capability transfer, call cancellation/teardown (including target AddressSpace retirement during a call), and any partial-completion states after visible effects.
- [ ] Complete architectural-state isolation beyond the selected TLS, timer/counter, PMU, debug-channel and FP/SIMD controls before protected EL0 claims.

## Phase 7 — Time and userspace scheduling

Reference: [Time contracts](capabilities-contract.md#time-and-userspace-scheduling). Prerequisites: D2/D4/D8/D9 (open questions in [`capabilities-decisions.md`](capabilities-decisions.md)).

- [ ] Separate Time-object storage from issuance of positive CPU budget; keep budget provenance and conservation.
- [ ] Implement Split/Merge/Query with explicit slots, checked results, transactional failures and no double accounting.
- [ ] Implement donation, preemption, cancellation and donor resumption through the shared completion mechanism.
- [ ] Implement Thread Start/Suspend/Resume with initialized contexts, explicit authority, legal transitions and preserved blocked completions and budget; never-returning self-retirement.
- [ ] Align consumed/remaining/activation/deadline observations with the scheduler records; charge PPC per-call time on Return once a real clock exists (`current_time_ns()` returns zero today).
- [ ] Provide client ownership APIs that preserve budget on pre-commit errors without relying on destructor side effects.
- [ ] Test conservation, insufficient budget, failed split/donation, incompatible merge, expiry, cancellation, nested delegation and simultaneous spending; run target scheduling tests with a userspace policy example.

**Exit:** storage cannot mint execution authority; delegated time is spent at most once; execution, observation and ownership agree across layers.

## Final integration

- [ ] Complete the init handoff: ELF-module AddressSpaces and Threads, their capacities and well-known grants with incarnation-bearing handoff records, Untyped delegation and accounting of boot/reserved memory, read-only module image capabilities, reclaimable boot memory and DTB-map removal ordering, dynamic stack placement, and EL0 entry through the context-switch path.
  - [ ] Replace the fixture init page with the init-handoff design (keys, parameters) once selected.
  - [ ] Copy a component's `.data` per instance before loading the same component twice; it is mapped in place from the bundle today.
  - [ ] Move the Bounce fixture (sync-test, ppc-test, via `libkicktest::bounce`) onto EL0 components where it does not need EL1 register access.
- [ ] Detect the Arm architecture version and optional features at runtime (`ID_AA64*_EL1`) and enable the protections the running core supports.
  - [ ] Enable PAN when `ID_AA64MMFR1_EL1.PAN` reports FEAT_PAN: set `PSTATE.PAN` and clear `SCTLR_EL1.SPAN`. Prerequisite: the EL1t boot Thread stops using its EL0-accessible low stack. Validate on a core with FEAT_PAN (QEMU `virt` with a newer CPU).
  - [ ] Select `CPTR_EL2.TZ`/`TSM` by feature: `libboot`'s `_startup_in_rust` sets them (RES1 on ARMv8.0); with SVE or SME they must be cleared so traps reach EL1 through `CPACR_EL1.ZEN`/`SMEN`.
- [ ] Replace the temporary `[patch.crates-io]` local path for `aarch64-cpu` (`CPTR_EL2` trap fields, `PMUSERENR_EL0`) with the released version once published.
- [ ] Harden the boot linear map: exclude kernel-image physical memory and replace blanket init RWX with per-segment permissions.
- [ ] Finish the `Domain` → `Thread`/`AddressSpace` rename in code (`CapError::InvalidDomain`, `DomainState` and the DCB types go with the D5 record replacement).
- [ ] Audit every wrapper against dispatch for false success or undocumented stubs, and run the full ABI, target-build and QEMU suites; record unavailable validation separately.
- [ ] After correctness is established, measure memory and performance costs and do a separate optimization pass without weakening the contracts.
