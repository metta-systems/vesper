# IOSpace

| | |
|---|---|
| Wire type | `0x85` (arch index 5) |
| Backing | None |
| Status | Not implemented |

## Purpose

An IOSpace will be the translation context a device sees for DMA, backed by an IOMMU (an SMMU on AArch64), so that devices can reach only the memory granted to them. Invoking an IOSpace capability fails with `UNSUPPORTED_ARCH_TYPE`.
