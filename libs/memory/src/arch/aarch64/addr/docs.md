## Arch-independent

Address is a 64-bit unsigned number (_address_ of a location in memory)

It can be aligned and checked for alignment.
There are not other limitations on the address.

## Arch-dependent

Address may have a size limitation, e.g. a 40-bits physical address on some platforms. - could be platform-dependent, not arch-dependent!

Address may contain additional information payload, for example ASID tags.
