# IOPort

| | |
|---|---|
| Wire type | `0x86` (arch index 6) |
| Pool | none |
| Status | x86-only kind; unsupported on AArch64 — dispatch returns `UnsupportedArchType` |

## Purpose

`IOPort` is the registered kind for x86 I/O port authority: the capability
family that would gate `in`/`out` port instructions on x86 targets. It exists
in the architecture catalogue because the catalogue is target-independent —
support depends on the target. An ID being defined does not make the kind
supported on a target whose architecture has no port I/O concept: on AArch64
the kind is meaningless and permanently unsupported.

## User-level visible operations

None, on any current target. On a future x86 target the intended vocabulary
would follow the seL4 `IOPort` model (read/write/consume operations on a port
range), but no contract is selected.

## Kernel-level implementation details

- `ArchType::IOPort` is defined in the catalogue
  (`libs/object/src/object_type.rs`); there is no object struct, pool,
  handler, or trait method for it on AArch64 — dispatch rejects it with
  `UnsupportedArchType`.
- The core/architecture separation is a dispatch and implementation boundary,
  not two competing invocation protocols: an x86 port operation would be an
  ordinary capability invocation dispatched through the arch arm.

## Sidenotes

- This kind is the canonical example of the numbering rule that "support
  depends on the target": registered ≠ supported.
- No AArch64 harm in keeping the ID: unknown/unsupported kinds fail
  explicitly with defined errors, and the catalogue stays shared across
  targets.

## TODOs

- Everything, contingent on an x86 target existing at all: operation schemas
  (read/write widths, port ranges), authority model, and trap/emulation
  mechanism (`io_bitmap` in the TSS on x86) — no decision is scheduled.

## Cross-reference: implementation vs. desired capabilities (🧠 Vesper vault)

- No vault note mentions I/O ports; the vault research is AArch64/RPi-focused
  (`aarch64 registers.md`, RPi platform code). The kind exists purely to
  keep the shared catalogue complete for other targets — no discrepancy to
  review, but also no desired-capability backing. If Vesper commits to
  AArch64-only, this kind could be a candidate for reassignment (an explicit
  decision, per the numbering rules).
