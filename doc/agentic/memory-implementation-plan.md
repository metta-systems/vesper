# Memory implementation plan

The checklist for memory types, cache maintenance, ASIDs and shared mappings (fbufs). The
rules are in [memory-design.md](memory-design.md#decisions), open questions only in
[memory-decisions.md](memory-decisions.md). Frame capability work (revocation, remapping,
reclamation) stays in Phase 5 of the
[capabilities plan](capabilities-implementation-plan.md); its working rules, validation
commands and `just` recipes apply here unchanged.

Mark an item `- [x]` only after implementation and validation have passed. A finished
item leaves a one-line marker in [memory-completed.md](memory-completed.md).

- [ ] Let RAM Frames map `WriteCombining` (Normal non-cacheable) for memory shared with non-coherent devices, with the direct-map alias settled as MEM1 selects.
- [ ] EL0 cache maintenance (`SCTLR_EL1.UCI`) for components that maintain shared buffers themselves, as MEM2 selects.
- [ ] Implement fbuf setup that agrees addresses for every participant before mapping, with reservation/conflict handling and rollback before pointers are published.
- [ ] Implement exclusive, immutable-shared and mutable-shared modes with explicit transitions, and `MappedSlice` ownership of a private, non-derivable capability and mapping with incarnation-checked Drop cleanup.
- [ ] Define per-target MMU/MPU/IOMMU, address-width and DMA restrictions, with a protected-context fallback that never downgrades hostile-code isolation.
- [ ] Implement `ASIDControl` pool creation and partitioning of the 16-bit ASID space once its schema is selected.
