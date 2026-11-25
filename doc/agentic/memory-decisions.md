# Memory open decisions

Parking lot for unanswered memory decisions. Each entry stays open until
the maintainer answers it explicitly. Answered decisions go into
[memory-design.md](memory-design.md#decisions); their tasks into
[memory-implementation-plan.md](memory-implementation-plan.md).

## MEM1. RAM Frames mapped `WriteCombining`

Memory shared with non-coherent devices could be mapped Normal non-cacheable instead of
maintained by cache operations. The kernel's direct map aliases all RAM as write-back,
and the same memory under two attributes is architecturally unpredictable. How is that
alias settled (unmap the range from the direct map, carve such Frames from a region the
direct map leaves out, or another way)?

## MEM2. EL0 cache maintenance

Should components that maintain shared buffers themselves get EL0 cache maintenance
(`SCTLR_EL1.UCI`), alongside or instead of the Frame cache maintenance operations?

## MEM3. ASID in high `VirtAddr` bits

Should ASID identity occupy high `VirtAddr` bits, or should that proposal be rejected?
