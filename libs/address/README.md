# libaddress

The types Address<Virtual> and Address<Physical> represent the addresses before and after the mapping in the MMU.

## Arch-independent

Address is a 64-bit unsigned number (_address_ of a location in memory)

It can be aligned and checked for alignment.
There are not other limitations on the address.

## Arch-dependent

Address may have a size limitation, e.g. a 40-bits physical address on some platforms. - could be platform-dependent, not arch-dependent!

Address may contain additional information payload, for example ASID tags.

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
