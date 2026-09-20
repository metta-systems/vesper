# VSpace

| | |
|---|---|
| Wire type | `0x82` (arch index 2) |
| Pool | `PoolTag::VSpace` (placeholder object exists; no instances created) |
| Status | Reserved: no handler, no creation; dispatch returns `UnsupportedArchType` |

## Purpose

`VSpace` was the registered kind for a separately targetable translation
context (seL4's VSpace / address-space object). Under the selected
mapping-context identity (2026-09-15) **the Domain is the mapping context**:
a mapping is recorded against the Domain that owns the translation-root
backend, and there is no separately targetable VSpace kernel object. Sharing
across protection boundaries uses frame capabilities mapped into each
participant's own Domain context (distinct PTEs); sharing a translation
context would merge protection boundaries, which the selected Domain =
VSpace boundary prohibits.

The registered kind ID remains reserved — deliberately not renumbered.
Assigning it any other role is a separate future decision.

## User-level visible operations

None. The kind is not creatable (`Untyped.Retype` rejects it), not granted at
bootstrap, and dispatch rejects it with `UnsupportedArchType`. Translation
operations that a VSpace would have carried are realized as:

| Intended VSpace operation | Actual realization |
|---|---|
| Install/activate a translation context | `PageTable.Map` (root, against a Domain cap) + `ASIDPool.Assign` + `Domain.Activate` |
| Map a frame into a context | `Frame.Map` with the target Domain capability |
| Withdraw a context | `PageTable.Unmap` (root) + `Domain.Retire` |

## Kernel-level implementation details

- `ArchType::VSPACE` is defined in the catalogue
  (`libs/object/src/object_type.rs`); `AArch64VSpace` is an empty placeholder
  struct with a pool tag (`kernel/nucleus/src/objects/arch/vspace.rs`) so the
  arch pool machinery stays total.
- `ArchObjects::invoke_vspace` exists as a deferred trait method; nothing
  calls it in practice since dispatch rejects the kind first.
- The pool tag (`PoolTag::VSpace = 17`) exists so the checked-identity
  machinery remains exhaustive; no object is ever allocated into it.

## Sidenotes

- Keeping the ID reserved (rather than reusing it) follows the numbering
  rule: a reserved ID does not advertise support, and reassigning meaning is
  an explicit decision, not a silent reuse.
- If a future target needs a separately targetable translation context (e.g.
  shared libraries across Domains without shared frames), this kind is where
  that decision would land.

## TODOs

- None active. Any activation of this kind requires: a contract for how a
  VSpace object differs from a Domain mapping context without violating D1's
  protection-boundary selection, plus full operation schemas (D4/D6/D9).

## Cross-reference: implementation vs. desired capabilities (🧠 Vesper vault)

- `Vesper.md` (vault): "**single address space** … pointer transparency
  between processes" and Mungi/SASOS references — **divergence**: a
  global SAS would make one VSpace the shared context for all domains; the
  selected D1 architecture instead gives each Domain its own context and
  shares via frames. The vault note itself concedes SAS "may be seen as an
  imposed policy decision" and that Vesper "does not dictate specific
  address space arrangements" — the current selection is the concrete
  resolution of that caveat, and it is why VSpace stays reserved.
- `Memory.md` (vault): "A passive address space … in which arbitrary threads
  may execute — a Protection Domain (PD)" (Mach/Mungi notes) — **partial
  alignment**: the Domain-as-context selection keeps the PD notion but drops
  the "arbitrary threads execute in one address space" decoupling of threads
  from address spaces; thread-vs-domain scheduling remains future work
  (Phase 7).
- `Prototype.md` (vault): no VSpace kind in the sketch — consistent with
  reserving rather than implementing.
