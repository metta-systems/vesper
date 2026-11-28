# Memory research

Prior art behind the [memory design](memory-design.md); not a specification.

## Frame memory attributes and write-combining (seL4, Composite, Linux)

Evidence behind the [MEMORY-4](memory-design.md#memory-4-frame-memory-attributes) decision (Frame memory attributes); the "Consequences" below were written before it and describe the MAIR as it was then. Sources read 2026-10-13: seL4 `master` (`src/arch/arm/64/kernel/vspace.c`, `src/arch/x86/kernel/vspace.c`), Composite `main` (`src/kernel/include/pgtbl.h`, `src/platform/{i386,armv7a}/chal_pgtbl.*`, `src/platform/i386/{vm,lapic}.c`), Linux `arch/arm64/include/asm/pgtable.h`.

### seL4

- The attribute is an argument of the mapping call (`Page_Map`/`Remap`, a `vm_attributes` word), not a property of the Frame or its Untyped.
- AArch64 knows two bits: `armPageCacheable` and `armExecuteNever`. In `makeUserPagePTE`, cacheable selects the `NORMAL` MAIR index (inner-shareable on SMP) and not cacheable selects `DEVICE_nGnRnE`. The MAIR enum also defines `DEVICE_nGnRE`, `DEVICE_GRE`, `NORMAL_NC` and `NORMAL_WT`, but this file never selects `NORMAL_NC`: AArch64 seL4 has no write-combining mapping, and "uncached" means Device-nGnRnE, as in Vesper today.
- x86 does have it: `init_pat_msr` programs PAT entry 4 as write-combining. The user-visible attribute names were not read.

### Composite

- Mapping flags are fixed by the kernel. User mappings get `PGTBL_USER_DEF` (x86) or `CAV7_4K_USER_DEF` (cacheable, bufferable); the only bits a component can change when delegating a mapping are `WRITABLE` (plus protection keys and `XDISABLE` on x86-64), and never upward.
- `PGTBL_NOCACHE` exists but only the kernel uses it (`device_map_mem` for the local APIC and ACPI tables). No user-selectable cacheability, no write-combining, no AArch64 port.

### Linux arm64 (the usual meaning on this architecture)

- `pgprot_writecombine` = `MT_NORMAL_NC` plus PXN|UXN: write-combining on AArch64 is Normal non-cacheable memory.
- `pgprot_noncached` = `MT_DEVICE_nGnRnE`; `pgprot_device` = `MT_DEVICE_nGnRE`.
- Normal-NC permits unaligned access and write merging; Device memory faults on unaligned access, which is why other kernels map framebuffers Normal-NC (Zephyr's `qemu_ramfb` change).

### Consequences for Vesper

- Neither microkernel gives a precedent to copy on ARM. seL4's per-mapping attribute word is the shape that fits the existing reserved `x5` of `Frame.Map`.
- Vesper's MAIR has index 0 Normal write-back, 1 Device-nGnRnE and 2 Device-nGnRE (`libs/memory/src/arch/aarch64/mmu.rs`); write-combining needs a Normal non-cacheable index added there.
- Mismatched attributes on one physical page are a hardware hazard; the no-intra-alias policy covers one AddressSpace, and the framebuffer is also read by the GPU, so the barrier discipline (`dsb` after writes) belongs to the component.
