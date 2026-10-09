---
name: capability-refactor
description: Guide one incremental Vesper capability refactor across shared ABI, userspace wrappers, kernel API, objects, and syscall entry; follow the canonical contract and checkbox plan, use JJ and Justfile workflows, resolve decision blockers, and validate a scoped cross-layer change.
---

# Capability refactor

## Start with the contract

- Resolve every repository-relative path below against the **Vesper repository root**, not this skill's directory (`.agents/skills/capability-refactor`).
- Always read **both** `doc/capabilities-contract.md` (canonical contract) and `doc/capabilities-implementation-plan.md` (checkbox plan) first, before analysis or edits. If either is unavailable, ask the user rather than inventing its contents.
- For lifetime, ownership, authority, delegation, revocation, or reclamation work, also read the relevant sections of `doc/capabilities-design.md` (why each rule exists, alternatives weighed) and `doc/capabilities-research.md` (prior-art evidence: seL4, Composite, Fluke, KeyKOS, NOVA and others). Neither holds tasks, decisions or status; they supplement the contract.
- Preserve user and concurrent-agent edits to these documents; coordinate overlapping changes and make focused updates rather than replacing either document wholesale.
- `doc/capabilities-decisions.md` is exclusively a parking lot for **unanswered architectural/contract decisions**. Do not keep resolved directions, settled decision recaps, implementation TODOs, code-audit tasks, or optional refactoring chores there. When a decision is resolved, record it in the canonical contract (and relevant lifetime/authority analysis), then remove the resolved item from `capabilities-decisions.md`. Put implementation, testing, and integration work in `doc/capabilities-implementation-plan.md`. The one exception is the **Future work** section at the end of `capabilities-decisions.md`: ideas the project might or might not pursue later (for example extended `x1..x7` IPC outputs). Future work needs no near-term decision. Do not present it as a decision blocker, do not let it constrain current contracts or tests, and do not plan around it. Describe each future-work idea only in that section. Elsewhere, at most leave a one-line link to it where the idea is relevant. Do not restate or paraphrase it across documents.
- Tasks live only in `doc/capabilities-implementation-plan.md`; open decisions only in `doc/capabilities-decisions.md`; selected contracts only in `doc/capabilities-contract.md`. Other documents (including `doc/capabilities-design.md`) link to them instead of restating them. When a plan item is finished, replace it with a one-line marker in `doc/capabilities-completed.md` (what exists and where it is exercised). Do not copy narratives, dated decision logs or replaced text anywhere: version control keeps the history.
- Distinguish accepted contracts, proposals, open decisions, and implementation status. Existing code and unchecked plan items are not architectural approval.
- Use the canonical `CoreType` numeric IDs, not legacy `ObjectType` constants. Core and architecture kind IDs are not compatibility constraints: the maintainer may reassign either catalogue to group kinds by functionality/category. Architecture wire IDs retain the high-bit category marker (`0x80`), but architecture-local indices may also be reassigned.
- Never renumber silently. For an approved regrouping, record the new core and/or architecture mapping in `doc/capabilities-contract.md` first, reconcile the implementation plan, then migrate the shared catalogue, dispatch, errors, literal ABI tests, and object-type documentation together. Do not preserve prior IDs, aliases, or reserved-slot assumptions as backwards compatibility unless the maintainer explicitly selects that policy.
- Treat disagreement between the canonical contract, `CoreType`/`ArchType`, and implementation as a discrepancy to resolve explicitly. Existing numeric mappings remain authoritative only until an approved contract change replaces them; do not infer that they are immutable.

## Standing practices

These apply to every slice and are never "done"; they are not plan checkboxes.

- **One design authority.** `doc/capabilities-contract.md` is the single consolidated reference. Record the maintainer's amendments there, as dated maintainer decisions, with matching updates to the analysis document and plan. Never revive the retired `kernel/nucleus/design.md`, and never start a second design document that competes with the contract.
- **Name each slice's prerequisite decisions.** Before implementing a slice, identify which D1–D9 decisions it depends on and confirm they are settled in the contract. Leave unrelated decisions explicitly open, parked in `doc/capabilities-decisions.md`, rather than blocking all progress on them or guessing their answers.
- **Keep sketches and unimplemented operations labeled.** Research examples, excluded sketches and unimplemented operations must be clearly labeled as such in code and documents. When the corresponding code is reconciled, remove the diagrams and comments that now conflict with it and look authoritative, subject to "Preserve design-intent comments" below.

## Keep object-types documentation current

`doc/object_types/` is the **user-facing technical reference** for the object kinds: what each kind is for and how to invoke it, as currently implemented. Write it for a component author, not for the people building the kernel. Update it in the same slice as any code change that affects a kind. When a page goes stale, rewrite the affected sections from the code; never append corrections, dated notes or "formerly…" asides to old text.

**Structure.**

- `README.md` has an intro (what the pages cover), **Wire encoding**, **Catalogue** (core and arch tables) and **Invocation model** (register ABI, dispatch diagram, one short paragraph on blocking and address-space switches). Per-kind detail never lives in the README.
- Catalogue status cells are one short line: which operations work, or that the kind is not implemented.
- Every per-kind page has, in this order: an H1 with the kind name; a header table (wire type; backing or associated object; one-line status); **Purpose**; **Operations** (an op table — op, name, arguments, required rights, result — then one subsection per operation with registers, behavior and errors); **Implementation** (only what a user benefits from knowing: backing, limits, observable effects). A short **Planned changes** section is allowed only for a significant, concrete planned change; describe it in a sentence or two. Omit any section with nothing in it. There are no Sidenotes or TODO sections.

**Content.**

- **Describe the current implementation.** No history (dates, "selected", "moved", "renamed", "replaced"), no change logs, no test logs, no planned work beyond a brief Planned changes section.
- **No internal process language.** No decision IDs (D1–D9), "maintainer decision", "provisional", "freezing", "contract", "selection", "slice", "fixture", "sketch", and no editorial asides such as "deliberate sentinel" or "through real SVC dispatch".
- **Plain technical statements.** State what something is and does. Do not use "not X, but Y" or "X, not Y" contrasts; say what it is, and state a limitation as its own sentence when the user needs it.
- **One home per fact.** State each rule once, on the page of the kind that owns it, and link from elsewhere: Invocation owns PPC stack extents, admission order and register state across a call; Thread owns `Thread.Return` and fault delivery; KeyTable owns the well-known slot table; the README owns status codes and the register ABI.
- **Be concise.** Prefer tables for registers, errors, outcomes and slot layouts.
- **Verify facts when touching a page.** Check wire IDs, op numbers, registers, rights, slot numbers, error statuses and function names against the code, and fix any error found anywhere on the page.

**Allowed references.**

- Other `doc/object_types/` pages, by anchors that exist. When renaming a heading, update every link to it.
- Source paths in this repository (`kernel/`, `libs/`, `userspace/`) and hardware or architecture concepts.
- **Not allowed:** any internal capability document (`doc/capabilities-*.md`), private research notes, and decision records. The capability documents are internal work documents and must not leak into the user documentation.

## No backwards compatibility

- There is no previous version of anything: once the maintainer approves a change, preserve no backwards compatibility, migration path, deprecated alias, or legacy behavior. Functionality can be completely ripped out and replaced when necessary; update all consumers (shared ABI, wrappers, kernel API, objects, tests, documentation) in the same slice rather than keeping the old version alongside the new one.
- This does not authorize silent divergence from approved contracts: update the contract and plan first, then remove the old behavior outright.

## Gate architectural decisions

Check the documents' decision status and dependencies for the selected item:

- **D1:** selected protection/threat model and shared-address-space semantics; remaining backend, machine-local namespace-conflict, and fault-delivery details. Multi-node global address allocation is out of scope.
- **D2:** selected management-authority/bookkeeping split; remaining revocation, completion, and reuse mechanisms.
- **D3:** ownership and lifetime.
- **D4:** authority, badges, and bootstrap.
- **D5:** domain control block (DCB).
- **D6:** memory and ASID management.
- **D7:** IPC and blocking.
- **D8:** time.
- **D9:** ABI evolution.

Resolve blocking decisions with the user before implementing dependent behavior. Present the concrete choice, consequences, and affected checklist item; never silently promote a proposal to an accepted contract. Present decision options as plain text in the conversation (markdown lists with lettered/named options), not via interactive input-request forms: the maintainer cannot read truncated option text in the selection UI and cannot make a choice there. Wait for the user's typed reply.

For lifecycle and authority changes, consult the **CONFIRMED**, **INTERIM**, and **OPEN / DECISION REQUIRED** notes in `doc/capabilities-design.md` and map them to D1–D9 above. Preserve the selected hostile-native-code confinement, capability-scoped trust, AddressSpace = protection boundary (the core Thread executes in an arch AddressSpace), shared numerical meaning/cheap fbufs with protected-context fallback, no-intra-AddressSpace-alias policy, incarnation, permission-based retirement, SPeCK-like KeyMaster, accepted-leak, Copy-not-Map, local-Unmap versus origin-cap-Revoke, and private mapping-guard directions. Revocation takes precedence over client borrows; unsafe mapped-memory caller obligations are the preferred direction, not a kernel safety exemption. Do not treat these as approval of remaining identity encodings, selective-revocation mechanisms, Frame Map/remap schemas, machine-local namespace conflicts, per-target enforcement, precise Rust/fbuf contracts, or completion ABIs. Use `doc/capabilities-research.md` as evidence, not as permission to substitute unrelated semantics. Record new approved choices in the canonical contract first, then reconcile the analysis document and implementation plan.

Preserve the approved management authority split: KeyTable capabilities/rights authorize direct derivation, installation, and management; recipients also own the bookkeeping obligations and join the TCB of that libOS composition. Multiple managers or hierarchies are userspace policy. KeyMaster is a role, not a kernel-special singleton; do not add mandatory central post-hoc registration or reopen this settled choice. Remaining operation schemas, rights bits, and composition-specific synchronization/recovery are implementation work.

Fbuf setup must establish suitable addresses for all participating AddressSpaces before mapping. Do not add multi-node global address allocation. Stale raw-pointer protection after VA reuse is future work (`doc/capabilities-decisions.md`), never a current blocker; the contract states the current rule and what it does not relax.

## Pick one incremental slice

1. Read the user's requested scope, both documents, relevant module declarations, and the actual call chain from userspace encoding through syscall entry/dispatch to API and object operations and completion.
2. Inspect `kernel/nucleus/src/api`, `kernel/nucleus/src/objects`, `libs/object`, and `libs/syscall`; discover the relevant architecture and syscall-entry files by following declarations and calls.
3. Distinguish compiled, reachable behavior from excluded sketches. These families are excluded because they do not compile as-is today, not because their intended functionality is rejected; include them in design and future implementation considerations. File presence does not mean implementation is active; never enable a sketch merely to make it compile or return fake success.
4. Select **one scoped checklist item** consistent with the user's request. Identify its prerequisites, decision approvals, acceptance criteria, and affected layers. Do not execute the whole backlog unprompted.
5. Preserve existing user and agent edits. For changed contracts, update the canonical document first after approval, then keep the plan consistent before changing code. Review-only work must not change checkboxes.
6. Implement the slice coherently: shared ABI definitions, userspace encoding/result decoding, checked rights and object transitions, and relevant tests together. Every successful capability operation must emit the established green-check semihosting trace (`✅ Type::Operation()`) after its state transition commits and before returning success; failures must not emit a success trace. Audit for the trace alongside the operation's error and success paths. Avoid half-migrated call sites or incompatible numeric representations.
7. Validate, audit the diff, and check off only verified completed tasks. Leave partial or blocked items unchecked and record the blocker; never mark unrun tests complete.

Respect the plan's ordering and explicit prerequisites:

1. Contracts, status audit, and decision approvals.
2. Shared ABI and host-testability.
3. Active console/syscall boundary.
4. Guarded capability storage and thread lifetime.
5. Memory vertical slice.
6. Deferred completion first, then notifications/event counts and PPC Invocation.
7. Time.
8. Final integration and documentation audit.

## Preserve design-intent comments

- Preserve pre-existing comments and documentation describing the intended (to-be) functionality of APIs and objects, including design sketches, examples, diagrams, and commented-out pseudocode. They record user design intent even when the implementation is incomplete or excluded.
- Do not erase, replace, or rewrite those comments merely because the behavior is unsupported, a decision remains open, or the current code differs. Do not substitute a stub/unsupported warning for the intended behavior or relocate the intent solely into another document.
- If clarification is needed, retain the original text and add a separate, clearly labeled implementation-status or contract-status note. Preserving intent does not promote it to an approved contract or claim it is implemented.
- Ask for explicit permission before removing or substantively changing pre-existing design-intent comments. During the final diff audit, check for accidental comment loss and restore it before completion.

## Correctness before representation optimization

- Follow the correctness-first rule in `doc/capabilities-contract.md` and `doc/capabilities-design.md` section 3. Do not preserve current kernel-object/key sizes, packing, or inline representations at the expense of consistent identity, authority, mapping, and lifetime semantics. Add/remove/expand fields and introduce shared records when needed.
- Defer size/performance optimization to a separate measured pass after correctness. Existing KeyEntry size targets are not a reason to compress away necessary state or avoid a correct design.
- Whenever layouts change, audit and update allocation/accounting, backing size/alignment, pool capacities, table strides, DCB record/page views, initialization bounds, shared ABI consumers, and layout assertions/tests together. Do not merely disable assertions. Respect hardware-defined formats and coordinate changes to agreed wire layouts/encodings; this rule does not authorize ABI renumbering or unrelated rewrites.

## Keep responsibilities separate

- **Shared ABI:** pure, host-testable definitions of IDs, operations, rights, layouts, encoding rules, and errors; no kernel state or hardware dependency.
- **Userspace (`libs/object`, `libs/syscall` as appropriate):** typed encoding, result decoding, ownership ergonomics, and transport wrappers. Phantom types improve ergonomics but are not a security proof.
- **Kernel API (`kernel/nucleus/src/api`):** checked decoding, capability lookup and authorization, and orchestration; do not hide object invariants here.
- **Objects (`kernel/nucleus/src/objects`):** state, lifetime, ownership, and invariant-preserving transitions.
- **Architecture code:** hardware mechanism, not implicit authority policy.
- **Syscall entry/transport:** marshal requests and results; perform transport completion only after object/capability guards are released. Keep deferred completion explicit rather than returning success before the required transition.

## Safety requirements

- Never fabricate lifetimes or references with unsafe code to bypass ownership or guard constraints; establish real backing-storage and thread lifetime guarantees. Consult `doc/capabilities-design.md` sections 1–3 and 5–6 for slot/object/thread reuse, guarded access, and direct-memory/DCB hazards; thin fallible handles do not justify unguarded references.
- Prevent rights amplification across lookup, derivation, transfer, and invocation; validate authority in the kernel regardless of wrapper types.
- Preserve resources and ownership on pre-commit failure through validation/reservation and rollback. Where the approved contract permits irreversible partial completion, expose it explicitly with recoverable bookkeeping; never silently lose or duplicate resources.
- Reject malformed or unsupported user input with defined errors, not panics, unchecked indexing, or fake success.
- Keep authority, badges, lifetime, revocation, and blocking semantics aligned with approved decisions; expose unresolved assumptions instead of encoding them as defaults. Use `doc/capabilities-design.md` sections 2 and 4–8 to review per-kind authority, delete/revoke/retire/reclaim distinctions, pending-operation outcomes, and hardware-safe reuse; generation invalidation alone is not completed reclamation.

## Nightly Rust features

Vesper is a playground for nightly Rust features. Using the latest nightly-only language or library features is allowed when needed for the selected refactor; do not introduce awkward stable-only workarounds merely to avoid a feature gate. Inform the user whenever a new `#![feature(...)]` gate is required, including in a test harness: name the feature, explain why it is needed, and note any toolchain compatibility impact. This permission does not require a separate approval for each feature, but ask before changing the configured toolchain or making an architectural/ABI decision to use it. Verify availability on the configured nightly and validate through the project recipes; nightly features do not relax safety requirements or approved contracts.

## Use project tooling

- **Vesper is a pure Rust repository. Write tooling in Rust.** Do not add or use Python, Ruby, JavaScript/TypeScript, or their runtimes for helper tools, code generation, test scaffolding, log parsing, or build/test workflows, including temporary or one-off scripts. Existing `just` recipes and minimal shell command orchestration remain the workflow entry points; implement any necessary helper logic in Rust.
- **Never use Perl**, including `perl -pe`/`perl -i` one-liners for search-and-replace. Use the dedicated file-editing tools, `sed`, or other standard shell tools instead.
- Prefer tests that validate their own results using Rust assertions and the existing test runner's exit status. Do not add external wrappers or success-marker parsers when the test itself can validate the behavior. Reuse existing infrastructure first; add a Rust helper only when genuinely necessary.
- Vesper uses **JJ for version control and `just` for build, test, lint, and related workflows**. Run recipes from the repository root.
- Read the current `Justfile` before selecting validation commands. Use `just --list` to discover public recipes and `just --dry-run <recipe>` to inspect expanded commands and prerequisites without executing them.
- This is a `no_std` embedded project: the recipes supply the custom target, `build-std`, board CPU/cfg flags, feature combinations, linker scripts, warning policy, and QEMU test runner. Do not reconstruct those commands by hand or substitute bare `cargo clippy`, `cargo test`, `cargo check`, or standalone `rustfmt` for project validation.
- Run lint validation as **`just clippy`**: its build prerequisite and embedded feature matrix also include `just clippy-object-host` for the capability host harness. `just clippy-pre-push` is a smaller default-feature check plus the same host-harness check, not equivalent to the full recipe. Use `just fmt-check` for formatting and `just lint` for formatting plus full embedded and host-tool linting.
- Use `just test` for the full test workflow, or a relevant documented subset (`just test-device`, `just test-chainboot`, `just test-host`). `just test-host` runs the capability ABI tests through `just test-object-host`, then `chainofcommand`. Use `just test-object-host` for the focused capability tests; its native-host dependencies currently require AArch64.
- Use `just build` with the recipe's documented board/features when needed; for example, `just build rpi3 qemu`. Consult the work plan's command table for scope and default behavior.
- If a focused ABI/host test lacks a recipe, propose adding one to `Justfile` as scoped work; do not silently bypass the convention. Treat explicitly approved ad hoc diagnostics as supplemental evidence, never as a replacement for the configured project checks.
- Inspect recipe dependencies and side effects. Do not run interactive/debug/flash/eject/setup/dependency-update recipes as validation; `just ci` starts with cleanup. In particular, `setup-local-dev` installs tools and changes Git hooks, contrary to the no-default-version-control-mutation rule.

## Validate and report

- Start with pure ABI tests (IDs, layouts, encode/decode, errors), then state/rights/lifetime models and failure atomicity, then relevant target/QEMU integration, all through appropriate `just` recipes. Match coverage to the slice and its acceptance criteria; narrower checks do not imply the broader workflow passed.
- Bound long-running commands with timeouts. Report exact commands, results, missing prerequisites, and timeouts; an unavailable target run is a validation blocker, not a pass.
- Before completion, compare changed contracts, code, tests, and plan status, and reconcile the affected `doc/object_types/` documents — replace stale content rather than amending it (see "Keep object-types documentation current"). For lifecycle/authority slices, also reconcile `doc/capabilities-design.md`, `doc/capabilities-decisions.md` and the plan; documentation alone does not complete implementation or validation tasks. Report the item addressed, affected paths, observed validation, remaining blockers, and next prerequisite without starting another slice.
- The user uses **JJ only**: no raw Git. Do not automatically perform version-control operations; use read-only JJ only if necessary. No commit/change creation, history mutation, branch/bookmark changes, or push by default, and no force rewriting.
