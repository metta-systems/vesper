# ASID

| | |
|---|---|
| Wire type | `0x84` (arch index 4) |
| Pool | `PoolTag::ASID` (placeholder object exists; no instances created) |
| Status | Reserved: no handler, no creation; binding goes through `ASIDPool.Assign` |

## Purpose

The `ASID` kind is the registered placeholder for a capability naming a single
hardware Address Space ID — the seL4 `ASIDControl`-adjacent slot in the
catalogue. Under the selected ASID model (2026-09-15) it stays **reserved with
no pool or handler**: a Domain's translation root receives an ASID through an
authorized `ASIDPool.Assign` invocation directly, and no separately
capability-addressed ASID object is needed for that flow.

## User-level visible operations

None. The kind is not creatable, not granted at bootstrap, and dispatch
rejects it with `UnsupportedArchType`. ASID binding is performed via
[`ASIDPool.Assign`](asid_pool.md) — see that document for the active
operation.

## Kernel-level implementation details

- `ArchType::ASID` is defined in the catalogue; `AArch64ASID` is an empty
  placeholder struct with a pool tag
  (`kernel/nucleus/src/objects/arch/asid.rs`) keeping the arch pool machinery
  total. No object is ever allocated into the pool.
- The registered kind deliberately stays reserved rather than being renumbered
  or repurposed: assigning it any other role is a separate future decision.

## Sidenotes

- If the seL4 two-level model (ASIDControl mints ASIDPool capabilities) is
  ever adopted, this kind is where a control capability would land; today
  the boot pool is the sole issuer and no control operation exists.

## TODOs

- None active. Activation would require: a contract for what a single-ASID
  capability authorizes (safe reuse? transfer? revocation of cached
  translations?), plus hardware-safe ASID reuse rules (D6) and operation
  schemas (D4/D9).

## Cross-reference: implementation vs. desired capabilities (🧠 Vesper vault)

- `seL4 Capabilities.md` / `API/seL4 API.md` (vault): seL4 has both
  `seL4_ARM_ASIDControl` and `seL4_ARM_ASIDPool` capabilities — **partial
  divergence**: Vesper implements only the pool kind; the control kind is
  reserved here. No vault note requires a per-ASID capability, so the
  reservation is a conservative placeholder rather than a conflict.
- `Prototype.md` (vault): `cap_asid_control_cap = 11` — numbering superseded
  by the canonical arch baseline (ASID = `0x84`).
