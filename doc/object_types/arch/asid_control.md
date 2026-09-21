# ASIDControl

| | |
|---|---|
| Wire type | `0x84` (arch index 4) |
| Pool | `PoolTag::ASIDControl` (placeholder object exists; no instances created) |
| Status | Reserved: no handler, no creation; binding goes through `ASIDPool.Assign` |

## Purpose

The `ASIDControl` kind (renamed from `ASID`, 2026-09-21) is the registered
placeholder for the capability-protected control point over the ASID
namespace — the seL4 `ASIDControl` equivalent, the expected home for pool
creation/partitioning. An ASID itself is a hardware naming value bound to an
[`AddressSpace`](address_space.md), not an independently capabilitied
object, so it must not be a wire type; the reserved `ASID` kind was replaced
by `ASIDControl` at the same index. Under the selected ASID model
(2026-09-15) the kind stays **reserved with no pool or handler**: an
AddressSpace's translation root receives an ASID through an authorized
`ASIDPool.Assign` invocation directly, and no control operation is needed
for that flow.

## User-level visible operations

None. The kind is not creatable, not granted at bootstrap, and dispatch
rejects it with `UnsupportedArchType`. ASID binding is performed via
[`ASIDPool.Assign`](asid_pool.md) — see that document for the active
operation.

## Kernel-level implementation details

- `ArchType::ASID_CONTROL` is defined in the catalogue;
  `AArch64ASIDControl` is an empty placeholder struct with a pool tag
  (`kernel/nucleus/src/objects/arch/asid_control.rs`) keeping the arch pool
  machinery total. No object is ever allocated into the pool.
- The kind deliberately stays reserved: its operation schema (seL4-style
  pool creation from Untyped, multi-pool partitioning of the 16-bit ASID
  space) is open (D6).

## Sidenotes

- If the seL4 two-level model (ASIDControl mints ASIDPool capabilities) is
  adopted, this kind is where the control capability lands; today the boot
  pool is the sole issuer and no control operation exists.

## TODOs

- None active. Activation would require: a contract for pool
  creation/partitioning (D6), hardware-safe ASID reuse rules, and operation
  schemas (D4/D9).

## Cross-reference: implementation vs. desired capabilities (🧠 Vesper vault)

- `seL4 Capabilities.md` / `API/seL4 API.md` (vault): seL4 has both
  `seL4_ARM_ASIDControl` and `seL4_ARM_ASIDPool` capabilities — **partial
  divergence**: Vesper implements only the pool kind; the control kind is
  reserved here. No vault note requires a per-ASID capability, so the
  reservation is a conservative placeholder rather than a conflict.
- `Prototype.md` (vault): `cap_asid_control_cap = 11` — numbering superseded
  by the canonical arch baseline (ASIDControl = `0x84`).
