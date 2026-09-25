# Null

| | |
|---|---|
| Wire type | `0x00` (core) |
| Pool | none — never a pooled object |
| Status | Active: never a usable capability |

## Purpose

`Null` is the canonical "no capability". It exists so that an empty or
invalidated slot has a typed representation and so that invoking a null
capability fails with a defined error instead of undefined behavior. It is
never a usable capability and cannot be created by `Untyped.Retype`.

## User-level visible operations

None. Any invocation of a `Null`-typed entry is rejected before operation
decoding:

| Behavior | Result |
|---|---|
| Invoke a `Null` entry | `NullCapability` (status 2) |

There is no operation numbering for this kind; no operation ID is or will be
valid on it.

## Kernel-level implementation details

- `CoreType::Null` is variant 0 of the core catalogue
  (`libs/object/src/object_type.rs`); `ObjectType::NULL` is its wire alias.
- Dispatch (`kernel/nucleus/src/api/mod.rs`, `core_invoke`) matches
  `CoreType::Null` first and returns `CapError::NullCapability` without
  touching any pool or payload.
- `KeyEntry::null()` produces the null entry used to vacate slots
  (`kernel/nucleus/src/api/key_entry.rs`). A null entry is never *valid*:
  `KeyTable::insert` rejects installing one (`NullCapability`), and
  `KeyTable::lookup` reports an invalidated slot through the shared
  `InconsistentKey` diagnostics rather than returning a null entry as usable.
- Slot 0 of the bootstrap layout is conventionally named Null
  (`nucleus_capabilities.md` § "Slot conventions" — a bootstrap sketch, not a
  contract).

## Sidenotes

- The null kind keeps the catalogue total: every wire value in the core range
  decodes to *some* kind, and "no capability" is representable without a
  sentinel outside the type system.
- `Null` is distinct from an *empty slot* (never issued, incarnation 0) and
  from an *invalidated slot* (issued before, entry vacated). Those are key
  states, not object types — see [key_table.md](key_table.md).

## TODOs

- None specific to this kind. Bootstrap slot conventions (which slot, if any,
  permanently holds Null) are part of D4's bootstrap-layout decision.

## Cross-reference: implementation vs. desired capabilities (🧠 Vesper vault)

- The vault's `Capabilities/Prototype.md` sketch lists `Null` as a `Key`
  variant and `cap_null_cap = 0` in an seL4-inspired numbering — consistent
  with the current wire ID 0. The old sketch's *other* numbers (untyped = 2,
  endpoint = 4, …) were superseded by the canonical numbering in
  `nucleus_capabilities.md`; only Null kept its value.
- The vault wiki notes describe null as "the slot may or may not contain a
  capability" — the current implementation additionally distinguishes
  *never-issued* and *invalidated* slots via incarnations, which the vault
  notes do not model. No conflict, just a refinement.
