---
name: capability-refactor
description: Guide one incremental Vesper capability refactor across shared ABI, userspace wrappers, kernel API, objects, and syscall entry; follow the capabilities contract and plan, or the area's design and plan for area work (SMP, FP/SIMD, USB, scheduling and the like), use JJ and Justfile workflows, resolve decision blockers, and validate a scoped cross-layer change.
---

# Capability refactor

## Start with the contract

- Load the `ponytail` skill (`.agents/skills/ponytail`) together with this one and follow it for the whole slice: the smallest change that fully solves the task, and a short reply that ends with what was skipped or not checked. It does not relax anything below: the contract, the decision gates, the tests and the documentation updates still apply in full.
- Resolve every repository-relative path below against the **Vesper repository root**, not this skill's directory (`.agents/skills/capability-refactor`).
- Always read **both** `doc/agentic/capabilities-contract.md` (canonical contract for capabilities) and `doc/agentic/capabilities-implementation-plan.md` (checkbox plan) first, before analysis or edits. For work in an area that has its own documents (see [Area documents](#area-documents)), also read that area's design and plan. If a document is unavailable, ask the user rather than inventing its contents.
- **Find every area a slice touches in the [document index](#document-index) and read that area's design and plan too.** The contract holds only capability semantics; many rules a capability slice depends on live in area designs (Frame, PageTable, ASID and Activate rules in `memory-design.md`, Time and Scheduler rules in `scheduling-design.md`, Meddle and Kickstart wiring in `components-design.md`). Not finding a rule in the contract does not mean it is undecided: check the index before treating anything as open.
- For lifetime, ownership, authority, delegation, revocation, or reclamation work, also read the relevant sections of `doc/agentic/capabilities-design.md` (why each rule exists, alternatives weighed) and `doc/agentic/capabilities-research.md` (prior-art evidence: seL4, Composite, Fluke, KeyKOS, NOVA and others). Neither holds tasks, decisions or status; they supplement the contract.
- Preserve user and concurrent-agent edits to these documents; coordinate overlapping changes and make focused updates rather than replacing either document wholesale.
- `doc/agentic/capabilities-decisions.md` is exclusively a parking lot for **unanswered architectural/contract decisions**. Do not keep resolved directions, settled decision recaps, implementation TODOs, code-audit tasks, or optional refactoring chores there. When a decision is resolved, record it in the canonical contract if it concerns capabilities themselves, or in the area's design document otherwise (and relevant lifetime/authority analysis), then remove the resolved item from `capabilities-decisions.md`. Put implementation, testing, and integration work in `doc/agentic/capabilities-implementation-plan.md`. The one exception is the **Future work** section at the end of `capabilities-decisions.md`: ideas the project might or might not pursue later (for example extended `x1..x7` IPC outputs). Future work needs no near-term decision. Do not present it as a decision blocker, do not let it constrain current contracts or tests, and do not plan around it. Describe each future-work idea only in that section. Elsewhere, at most leave a one-line link to it where the idea is relevant. Do not restate or paraphrase it across documents.
- Capability tasks live only in `doc/agentic/capabilities-implementation-plan.md`, capability open decisions only in `doc/agentic/capabilities-decisions.md`, selected capability contracts only in `doc/agentic/capabilities-contract.md`. An area's tasks, open decisions and selected rules live only in that area's own documents (see [Area documents](#area-documents)); the capabilities plan links to the area plan in one line. Other documents (including `doc/agentic/capabilities-design.md`) link to them instead of restating them. When a plan item is finished, replace it with a one-line marker in `doc/agentic/capabilities-completed.md` (what exists and where it is exercised). Do not copy narratives, dated decision logs or replaced text anywhere: version control keeps the history. **NEVER extend or rewrite an existing item in a `*-completed.md` file** (`capabilities-completed.md`, `usb-completed.md`, and the like) when later work changes or builds on it: the file must show at a glance what was completed, and an item grown by successive edits stops being readable. Add a new one-line marker for the new completed work instead, and if an old marker became wrong, correct it in place with the smallest change, without adding narrative.
- Distinguish accepted contracts, proposals, open decisions, and implementation status. Existing code and unchecked plan items are not architectural approval.
- ? Use the canonical `CoreType`/`ArchType` numeric IDs, not legacy `ObjectType` constants. Core and architecture kind IDs are not compatibility constraints: the maintainer may reassign either catalogue to group kinds by functionality/category. Architecture wire IDs retain the high-bit category marker (`0x80`), but architecture-local indices may also be reassigned.
- Never renumber silently. For an approved regrouping, record the new core and/or architecture mapping in `doc/agentic/capabilities-contract.md` first, reconcile the implementation plan, then migrate the shared catalogue, dispatch, errors, literal ABI tests, and object-type documentation together. Do not preserve prior IDs, aliases, or reserved-slot assumptions as backwards compatibility unless the maintainer explicitly selects that policy.
- Treat disagreement between the canonical contract, `CoreType`/`ArchType`, and implementation as a discrepancy to resolve explicitly. Existing numeric mappings remain authoritative only until an approved contract change replaces them; do not infer that they are immutable.

## Area documents

Work that is not about capabilities themselves (SMP, FP/SIMD, USB, input, audio, logging, the compositor and displays, scheduling policy, and the like) has its own set of documents in `doc/agentic/`, named by the area:

| File | Holds | Same rules as |
|---|---|---|
| `<area>-design.md` | The area's contract: a **Decisions** section with the dated maintainer decisions, then the mechanism and its reasoning | `capabilities-contract.md` |
| `<area>-decisions.md` | Only unanswered questions, with options and a marked recommendation | `capabilities-decisions.md` |
| `<area>-implementation-plan.md` | Only open tasks, in order, as checkboxes | `capabilities-implementation-plan.md` |
| `<area>-completed.md` | One-line markers of finished tasks | `capabilities-completed.md` |

- Some areas use other names or extra background files for historical reasons (see the index). Follow what an area already has; do not rename its files unprompted.
- A new area gets the files it needs when the maintainer starts it, and the capabilities plan gets a one-line pointer to its plan.
- Each fact has one home. An area document links to the capabilities contract for capability semantics (kinds, keys, rights, badges, invocation ABI, lifecycle) and does not restate them. The capabilities contract keeps no pointer sections or summaries of an area; a capability rule that refers to an area rule links to the area design inline.
- **The capabilities contract is shrinking, aggressively.** Material in it that is not about capabilities themselves moves to the matching area design, one section at a time as the maintainer selects it, each move its own change: move the text without loss of rules, leave no stub heading or pointer behind, point every inbound link (in all documents) at the new home, drop the topic's row from the decision register, and move the related open questions (with their Future work ideas) and completed markers into the area's decisions and completed files. Do not move sections the maintainer has not selected, and never add new non-capability sections to the contract.

### Document index

Every document in `doc/agentic/` belongs to one row. **Keep this table current:** a change that adds, renames, splits, merges or removes a document or an area in `doc/agentic/`, or moves a topic from one area to another, updates this table (and the "Covers" column) in the same change. A file in `doc/agentic/` that no row names is a defect; add it to its row before continuing.

| Area (prefix) | Covers | Files |
|---|---|---|
| `capabilities` | Capability semantics: kinds and IDs, keys, rights, badges, invocation ABI, Retype, lifecycle, IPC and PPC, IRQ delivery, Thread and AddressSpace | `-contract`, `-decisions`, `-implementation-plan`, `-completed`; background `-design` (rationale), `-research` (prior art) |
| `memory` | Mapping identity, PageTable and Frame installation, rights and W^X, memory attributes, cache maintenance, physical aliases, ASIDs, Activate, translation installation, fbufs and MappedSlice | `-design`, `-decisions`, `-implementation-plan`, `-completed` |
| `scheduling` | Time, the root and child Schedulers, Thread configuration and start, scheduler-shared records, userspace scheduling policy | `-design`, `-decisions`, `-implementation-plan`, `-completed`; background `capability-scheduler-design.md`, `capabilities-scheduling.md`, `capabilities-time.md` |
| `components` | Meddle interfaces and imports, Kickstart's wiring (device memory, mailbox, PCI, data areas, channels), the shell, component loading, driver components | `-design`, `-decisions`, `-implementation-plan`, `-completed` |
| `smp` | Secondary cores, the kernel lock, per-core state, IPIs | `-design`, `-decisions`, `-implementation-plan`, `-completed` |
| `simd` | FP/SIMD per AddressSpace (the architectural-state rules are still in the capabilities contract) | `-design`, `-implementation-plan`, `-completed` |
| `usb` | USB host controllers and the USB stack | `-design`, `-decisions`, `-implementation-plan`, `-completed` |
| `input` | Input devices and events | `-design`, `-decisions`, `-implementation-plan`, `-completed` |
| `audio` | Audio output (PWM jack, DMA, `cprman` clocks), mixing and synthesis | `-design`, `-implementation-plan`, `-completed`; background `audio-profiling.md` |
| `logging` | The log server: several components sharing one PL011 | `-design`, `-implementation-plan`, `-completed` |
| `ui-compositor` | The compositor, windows and surfaces | `-design`, `-implementation-plan`, `-completed`; `multi-display-design.md` (several outputs) |
| Background only | Prior art and shelved designs; no rules, tasks or decisions | `irq-controller-placement.md`, `future-libmmio-design.md`, `brands-history.md` |
| Maintainer notes | `questions.md`: the maintainer's scratch notes; read, never edit unprompted | `questions.md` |

## Standing practices

These apply to every slice and are never "done"; they are not plan checkboxes.

- **One authority per topic.** `doc/agentic/capabilities-contract.md` is the single reference for capabilities themselves; each area's design document is the single reference for that area. Record the maintainer's amendments in the one that owns the topic, as dated maintainer decisions, with matching updates to its plan. Never put an area decision into the capabilities contract, and never start a document that competes with the owner of a topic.
- **Name each slice's prerequisite decisions.** Before implementing a slice, identify which decisions it depends on (D1–D9 for capabilities, the area's own IDs otherwise) and confirm they are settled in the document that owns them. Leave unrelated decisions explicitly open, parked in the matching decisions file, rather than blocking all progress on them or guessing their answers.
- **Keep sketches and unimplemented operations labeled.** Research examples, excluded sketches and unimplemented operations must be clearly labeled as such in code and documents. When the corresponding code is reconciled, remove the diagrams and comments that now conflict with it and look authoritative, subject to "Preserve design-intent comments" below.

## Keep object-types documentation current

`doc/object_types/` is the **user-facing technical reference** for the object kinds: what each kind is for and how to invoke it, as currently implemented. Write it for an external component author, not for the people building the kernel. Update it in the same slice as any code change that affects a kind. When a page goes stale, rewrite the affected sections from the code; never append corrections, dated notes or "formerly..." asides to old text.

**Structure.**

- `README.md` has an intro (what the pages cover), **Wire encoding**, **Catalogue** (core and arch tables) and **Invocation model** (register ABI, dispatch diagram, one short paragraph on blocking and address-space switches). Per-kind detail never lives in the README.
- Catalogue status cells are one short line: which operations work, or that the kind is not implemented.
- Every per-kind page has, in this order: an H1 with the kind name; a header table (wire type; backing or associated object; one-line implemented operations list; one-line status if applicable); **Purpose**; **Operations** (an op table — op, name, required rights, arguments, result — then one subsection per operation with registers, behavior and errors); **Implementation** (only what a user benefits from knowing: backing, limits, observable effects). A short **Planned changes** section is allowed only for a significant, concrete planned change; describe it in a sentence or two. Omit any section with nothing in it.

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
- **Not allowed:** any internal capability document (`doc/agentic/capabilities-*.md`), private research notes, and decision records. The capability documents are internal work documents and must not leak into the user documentation.

## No backwards compatibility

- There is no previous version of anything: once the maintainer approves a change, preserve no backwards compatibility, migration path, deprecated alias, or legacy behavior. Functionality can be completely ripped out and replaced when necessary; update all consumers (shared ABI, wrappers, kernel API, objects, tests, documentation) in the same slice rather than keeping the old version alongside the new one.
- This does not authorize silent divergence from approved contracts: update the contract and plan first, then remove the old behavior outright.
- **Meddle interfaces are prototypes**. Nothing built on them is in production, so a change to an interface (`<name>_vN.if`: adding, renaming, reordering or removing functions, or changing parameters, results or exceptions) edits that version in place. Do not add a `<name>_v(N+1)` file, keep the old functions, or bump the version number. A version bump is for an interface the maintainer has declared to have production users.

## Gate architectural decisions

Check the documents' decision status and dependencies for the selected item:

- **D1:** protection/threat model and shared-address-space semantics; backend, machine-local namespace-conflict, and fault-delivery details. Multi-node global address allocation is out of scope.
- **D2:** management-authority/bookkeeping split; remaining revocation, completion, and reuse mechanisms.
- **D3:** ownership and lifetime.
- **D4:** authority, badges, and bootstrap.
- **D5:** scheduler-shared records (rules in `scheduling-design.md`, open questions in `scheduling-decisions.md`).
- **D6:** memory and ASID management (mapping, ASID and fbuf rules in `memory-design.md`, open questions in `memory-decisions.md`).
- **D7:** IPC and blocking.
- **D8:** time (rules in `scheduling-design.md`, open questions in `scheduling-decisions.md`).
- **D9:** ABI evolution.

Resolve blocking decisions with the user before implementing dependent behavior. Present the concrete choice, consequences, and affected checklist item; never silently promote a proposal to an accepted contract. Present decision options as plain text in the conversation (markdown lists with lettered/named options), not via interactive input-request forms: the maintainer cannot read truncated option text in the selection UI and cannot make a choice there. Wait for the user's typed reply.

**Never assume an answer the maintainer has not explicitly given.** Record a decision only for the questions the reply names or unambiguously answers. A reply to a related point, a follow-up question, an "ok" to something else, or silence is not an answer to a pending question: it does not select the recommended option or any other. When a reply covers only some of the open questions, record those, keep the rest open in the matching decisions file (`doc/agentic/capabilities-decisions.md` or the area's), and ask again for the unanswered ones. If it is unclear whether a reply answers a question, ask instead of recording it. Recording an assumed answer as a "maintainer decision" and later undoing it is costly; this has happened repeatedly.

For lifecycle and authority changes, consult the **CONFIRMED**, **INTERIM**, and **OPEN / DECISION REQUIRED** notes in `doc/agentic/capabilities-design.md` and map them to D1–D9 above. Preserve the selected hostile-native-code confinement, capability-scoped trust, AddressSpace = protection boundary (the core Thread executes in an arch AddressSpace), shared numerical meaning/cheap fbufs with protected-context fallback, no-intra-AddressSpace-alias policy, incarnation, permission-based retirement, SPeCK-like KeyMaster, accepted-leak, Copy-not-Map, local-Unmap versus origin-cap-Revoke, and private mapping-guard directions. Revocation takes precedence over client borrows; unsafe mapped-memory caller obligations are the preferred direction, not a kernel safety exemption. Do not treat these as approval of remaining identity encodings, selective-revocation mechanisms, Frame Map/remap schemas, machine-local namespace conflicts, per-target enforcement, precise Rust/fbuf contracts, or completion ABIs. Use `doc/agentic/capabilities-research.md` as evidence, not as permission to substitute unrelated semantics. Record new approved choices in the document that owns the topic first (the capabilities contract for capability semantics, the area's design otherwise), then reconcile the analysis document and implementation plan.

Preserve the approved management authority split: KeyTable capabilities/rights authorize direct derivation, installation, and management; recipients also own the bookkeeping obligations and join the TCB of that libOS composition. Multiple managers or hierarchies are userspace policy. KeyMaster is a role, not a kernel-special singleton; do not add mandatory central post-hoc registration or reopen this settled choice. Remaining operation schemas, rights bits, and composition-specific synchronization/recovery are implementation work.

Fbuf setup must establish suitable addresses for all participating AddressSpaces before mapping. Do not add multi-node global address allocation. Stale raw-pointer protection after VA reuse is future work (`doc/agentic/capabilities-decisions.md`), never a current blocker; the contract states the current rule and what it does not relax.

## Pick one incremental slice

1. Read the user's requested scope, both documents, relevant module declarations, and the actual call chain from userspace encoding through syscall entry/dispatch to API and object operations and completion.
2. Inspect `kernel/nucleus/src/api`, `kernel/nucleus/src/objects`, `libs/object`, and `libs/syscall`; discover the relevant architecture and syscall-entry files by following declarations and calls.
3. Distinguish compiled, reachable behavior from excluded sketches. These families are excluded because they do not compile as-is today, not because their intended functionality is rejected; include them in design and future implementation considerations. File presence does not mean implementation is active; never enable a sketch merely to make it compile or return fake success.
4. Select **one scoped checklist item** consistent with the user's request. Identify its prerequisites, decision approvals, acceptance criteria, and affected layers. Do not execute the whole backlog unprompted.
5. Preserve existing user and agent edits. For changed contracts, update the owning document (capabilities contract or area design) first after approval, then keep the plan consistent before changing code. Review-only work must not change checkboxes.
6. Implement the slice coherently: shared ABI definitions, userspace encoding/result decoding, checked rights and object transitions, and relevant tests together. Every successful capability operation must emit the established green-check semihosting trace (`✅ Type::Operation()`) after its state transition commits and before returning success; failures must not emit a success trace. Audit for the trace alongside the operation's error and success paths. Avoid half-migrated call sites or incompatible numeric representations.
7. Validate, audit the diff, and check off only verified completed tasks. Leave partial or blocked items unchecked and record the blocker; never mark unrun tests complete.

Respect the plan's ordering and explicit prerequisites:

1. Contracts, status audit, and decision approvals.
2. Shared ABI and host-testability.
3. Active console/syscall boundary.
4. Guarded capability storage and thread lifetime.
5. Memory vertical slice (rules in `memory-design.md`, tasks split between Phase 5 and `memory-implementation-plan.md`).
6. Deferred completion first, then notifications/event counts and PPC Invocation.
7. Time and userspace scheduling (tracked in `doc/agentic/scheduling-implementation-plan.md`, rules in `scheduling-design.md`).
8. Final integration and documentation audit.

## Preserve design-intent comments

- Preserve pre-existing comments and documentation describing the intended (to-be) functionality of APIs and objects, including design sketches, examples, diagrams, and commented-out pseudocode. They record user design intent even when the implementation is incomplete or excluded.
- Do not erase, replace, or rewrite those comments merely because the behavior is unsupported, a decision remains open, or the current code differs. Do not substitute a stub/unsupported warning for the intended behavior or relocate the intent solely into another document.
- If clarification is needed, retain the original text and add a separate, clearly labeled implementation-status or contract-status note. Preserving intent does not promote it to an approved contract or claim it is implemented.
- Ask for explicit permission before removing or substantively changing pre-existing design-intent comments. During the final diff audit, check for accidental comment loss and restore it before completion.

## Correctness before representation optimization

- Follow the correctness-first rule in `doc/agentic/capabilities-contract.md` and `doc/agentic/capabilities-design.md` section 3. Do not preserve current kernel-object/key sizes, packing, or inline representations at the expense of consistent identity, authority, mapping, and lifetime semantics. Add/remove/expand fields and introduce shared records when needed.
- Defer size/performance optimization to a separate measured pass after correctness. Existing KeyEntry size targets are not a reason to compress away necessary state or avoid a correct design.
- Whenever layouts change, audit and update allocation/accounting, backing size/alignment, pool capacities, table strides, scheduler-shared record views, initialization bounds, shared ABI consumers, and layout assertions/tests together. Do not merely disable assertions. Respect hardware-defined formats and coordinate changes to agreed wire layouts/encodings; this rule does not authorize ABI renumbering or unrelated rewrites.

## Keep responsibilities separate

- **Shared ABI:** pure, host-testable definitions of IDs, operations, rights, layouts, encoding rules, and errors; no kernel state or hardware dependency.
- **Userspace (`libs/object`, `libs/syscall` as appropriate):** typed encoding, result decoding, ownership ergonomics, and transport wrappers. Phantom types improve ergonomics but are not a security proof.
- **Kernel API (`kernel/nucleus/src/api`):** checked decoding, capability lookup and authorization, and orchestration; do not hide object invariants here.
- **Objects (`kernel/nucleus/src/objects`):** state, lifetime, ownership, and invariant-preserving transitions.
- **Architecture code:** hardware mechanism, not implicit authority policy.
- **Syscall entry/transport:** marshal requests and results; perform transport completion only after object/capability guards are released. Keep deferred completion explicit rather than returning success before the required transition.

## Safety requirements

- Never fabricate lifetimes or references with unsafe code to bypass ownership or guard constraints; establish real backing-storage and thread lifetime guarantees. Consult `doc/agentic/capabilities-design.md` sections 1–3 and 5–6 for slot/object/thread reuse, guarded access, and direct-memory and shared-record hazards; thin fallible handles do not justify unguarded references.
- Prevent rights amplification across lookup, derivation, transfer, and invocation; validate authority in the kernel regardless of wrapper types.
- Preserve resources and ownership on pre-commit failure through validation/reservation and rollback. Where the approved contract permits irreversible partial completion, expose it explicitly with recoverable bookkeeping; never silently lose or duplicate resources.
- Reject malformed or unsupported user input with defined errors, not panics, unchecked indexing, or fake success.
- Keep authority, badges, lifetime, revocation, and blocking semantics aligned with approved decisions; expose unresolved assumptions instead of encoding them as defaults. Use `doc/agentic/capabilities-design.md` sections 2 and 4–8 to review per-kind authority, delete/revoke/retire/reclaim distinctions, pending-operation outcomes, and hardware-safe reuse; generation invalidation alone is not completed reclamation.

## Nightly Rust features

Vesper is a playground for nightly Rust features. Using the latest nightly-only language or library features is allowed when needed for the selected refactor; do not introduce awkward stable-only workarounds merely to avoid a feature gate. Inform the user whenever a new `#![feature(...)]` gate is required, including in a test harness: name the feature, explain why it is needed, and note any toolchain compatibility impact. This permission does not require a separate approval for each feature, but ask before changing the configured toolchain or making an architectural/ABI decision to use it. Verify availability on the configured nightly and validate through the project recipes; nightly features do not relax safety requirements or approved contracts.

## Use project tooling

- **Vesper is a pure Rust repository. Write tooling in Rust.** Do not add or use Python, Ruby, JavaScript/TypeScript, or their runtimes for helper tools, code generation, test scaffolding, log parsing, or build/test workflows, including temporary or one-off scripts. Existing `just` recipes and minimal shell command orchestration remain the workflow entry points; implement any necessary helper logic in Rust.
- **Dependencies between workspace crates are `{ workspace = true }`, never a relative `path = "../.."`.** Declare each member crate once in the root `[workspace.dependencies]` (with its `package = ...` when the key differs) and refer to it by that key from every other member. A relative path in a member's `Cargo.toml` is only for crates outside the workspace (such as `tools/if_lsp`). When you add a crate, add its workspace entry in the same change, and when you move one, update the single root entry.
- **Never use Perl, Python or `sed`** (including `perl -pe`/`perl -i`, `python -c` and `sed -i` one-liners) to edit files or do search-and-replace. Make every file change with the `rust-mcp-filesystem` MCP tools when they are connected (`mcp__filesystem__edit_file` for edits, `mcp__filesystem__write_file` for new files or full rewrites); fall back to the built-in Edit/Write tools only when the MCP server is unavailable. BSD `sed` silently fails on multi-line and `a`/`c`/`i` commands, and a failed or half-applied script corrupts files unnoticed. Read-only inspection with `sed -n` ranges is fine; prefer the Read tool.
- Prefer tests that validate their own results using Rust assertions and the existing test runner's exit status. Do not add external wrappers or success-marker parsers when the test itself can validate the behavior. Reuse existing infrastructure first; add a Rust helper only when genuinely necessary.
- Vesper uses **JJ for version control** (read the `jujutsu` skill) and **`just` for build, test, lint, and related workflows** (read the `just` skill).
- Run just recipes from the repository root. The user uses **JJ only**: no raw Git. Do not automatically perform version-control operations; use read-only JJ only if necessary. No commit/change creation, history mutation, branch/bookmark changes, or push by default, and no force rewriting.
- Read the current `Justfile` before selecting validation commands. Use `just --list` to discover public recipes and `just --dry-run <recipe>` to inspect expanded commands and prerequisites without executing them.
- This is a `no_std` embedded project: the recipes supply the custom target, `build-std`, board CPU/cfg flags, feature combinations, linker scripts, warning policy, and QEMU test runner. Do not reconstruct those commands by hand or substitute bare `cargo clippy`, `cargo test`, `cargo check`, or standalone `rustfmt` for project validation.
- Run lint validation as **`just clippy`**: its build prerequisite and embedded feature matrix also include `just _clippy-object-host` for the capability host harness. `just _clippy-pre-push` is a smaller default-feature check plus the same host-harness check, not equivalent to the full recipe. Use `just fmt-check` for formatting and `just lint` for formatting plus full embedded and host-tool linting.
- Use `just test` for the full test workflow, or a relevant documented subset (`just _test-device`, `just _test-chainboot`, `just test-host`). `just test-host` runs the capability ABI tests through `just _test-object-host`, then `chainofcommand`. Use `just _test-object-host` for the focused capability tests; its native-host dependencies currently require AArch64.
- Use `just build` with the recipe's documented board/features when needed; for example, `just build rpi3 qemu`. Consult the work plan's command table for scope and default behavior.
- If a focused ABI/host test lacks a recipe, propose adding one to `Justfile` as scoped work; do not silently bypass the convention. Treat explicitly approved ad hoc diagnostics as supplemental evidence, never as a replacement for the configured project checks.
- Inspect recipe dependencies and side effects. Do not run interactive/debug/flash/eject/setup/dependency-update recipes as validation; `just ci` starts with cleanup. In particular, `setup-local-dev` installs tools and changes Git hooks, contrary to the no-default-version-control-mutation rule.

## Validate and report

- Start with pure ABI tests (IDs, layouts, encode/decode, errors), then state/rights/lifetime models and failure atomicity, then relevant target/QEMU integration, all through appropriate `just` recipes. Match coverage to the slice and its acceptance criteria; narrower checks do not imply the broader workflow passed.
- Bound long-running commands with timeouts. Report exact commands, results, missing prerequisites, and timeouts; an unavailable target run is a validation blocker, not a pass.
- Before completion, compare changed contracts, code, tests, and plan status, and reconcile the affected `doc/object_types/` documents — replace stale content rather than amending it (see "Keep object-types documentation current"). For lifecycle/authority slices, also reconcile `doc/agentic/capabilities-design.md`, `doc/agentic/capabilities-decisions.md` and the plan; documentation alone does not complete implementation or validation tasks. Report the item addressed, affected paths, observed validation, remaining blockers, and next prerequisite without starting another slice.
