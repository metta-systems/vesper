# libaddress

The types Address<Virtual> and Address<Physical> represent the addresses before and after the mapping in the MMU.

## Exports

This library should export the following:

- Type `Address<Physical>` representing the physical memory address, independent of the target platform
  (plus some shared operations like NumOps).
- Type `PhysAddr` specific to the target platform, with size and content limitations.
- Type `Address<Virtual>` representing the virtual memory address, independent of the target platform
- plus some shared operations like NumOps or ToPointer/FromPointer ops.
- Type `VirtAddr` specific to the target platform (e.g. with_asid() on aarch64)

----

For more information please re-read.
