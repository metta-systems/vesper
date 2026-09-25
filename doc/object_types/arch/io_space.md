# IOSpace

| | |
|---|---|
| Wire type | `0x85` (arch index 5) |
| Pool | none |
| Status | Deferred: dispatch returns `UnsupportedArchType` |

## Purpose

`IOSpace` is the registered kind for an I/O address space / device-translation
context — the capability family that would gate DMA-capable device access to
memory (an SMMU/IOMMU-backed translation context on AArch64). It exists in the
catalogue because D1's selected protection architecture includes "DMA
mediation/IOMMU" as a protection requirement: devices must not be able to
bypass protection boundaries by writing memory directly.

## User-level visible operations

None. The kind is not creatable (`Untyped.Retype` rejects it), not granted at
bootstrap, and dispatch rejects it with `UnsupportedArchType`
(`ArchObjects::invoke_io_space` provides the default rejection).

## Kernel-level implementation details

- `ArchType::IOSpace` is defined in the catalogue
  (`libs/object/src/object_type.rs`); no object struct, pool, or handler
  exists beyond the default trait rejection in
  `kernel/nucleus/src/objects/arch_objects.rs`.
- No IOMMU/SMMU driver or mediation path exists in the kernel today.

## Sidenotes

- The selected D1 architecture defers side channels but keeps DMA
  mediation/IOMMU as a protection requirement; this kind is the catalogue
  anchor for that requirement.
- Device memory policy more broadly (device Untypeds, device frames) is D6;
  whatever IOSpace becomes, its authority must originate from authorized
  hardware-resource assignment, not arbitrary retype.

## TODOs

- Everything: whether IOSpace is an SMMU context object, how device
  assignments are authorized, how DMA completion interacts with the
  kernel-mediated release/acquire memory-ordering contract (devices carry
  their own cache-coherency and completion obligations — a syscall alone does
  not discharge them), and the operation schemas — D1/D6/D9.
- Requires target hardware support (SMMU on AArch64) before any slice can
  land.

## Cross-reference: implementation vs. desired capabilities (🧠 Vesper vault)

- `Vesper.md` (vault): "Interrupts come from hardware … kernel is responsible
  for translating them into invocations of the device drivers' handlers" —
  related but distinct: that note is about interrupt delivery (see
  [irq_handler.md](irq_handler.md)); DMA *access* mediation is the IOSpace
  half. Neither is implemented.
- `Memory.md` (vault): device memory as a distinct untyped type with
  kernel-enforced usage restrictions — **direction consistent** (device
  sources are rejected by Retype today), but the vault has no IOMMU/IOSpace
  concept; the DMA-mediation requirement comes from the D1 selection in
  `nucleus_capabilities.md` instead.
- No vault note covers device-to-memory translation; this kind currently has
  no desired-capability design behind it beyond the one-line D1 requirement —
  worth a vault note of its own before any implementation work starts.
