The Device memory type has several attributes:

    G or nG - Gathering or non-Gathering. Multiple accesses to a device can be merged into a single transaction except for operations with memory ordering semantics, for example, memory barrier instructions, load acquire/store release.

    R or nR - Reordering.

    E or nE - Early Write Acknowledge (similar to bufferable).

Only four combinations of these attributes are valid:

    Device-nGnRnE  <-- "Strongly Ordered"
    Device-nGnRE   <-- "Device Memory"
    Device-nGRE
    Device-GRE

Typically peripheral control registers must be either `Device-nGnRE`, or `Device-nGnRnE`. This prevents reordering of the transactions in the programming sequences.

`Device-nGRE` and `Device-GRE` memory types can be useful for peripherals where memory access sequence and ordering does not affect results, for example, in bitmap or display buffers in a display interface. If the bus interface of such peripheral can only accept certain transfer sizes, the peripheral must be set to `Device-nGRE`.

Device memory is shareable, and must be cached.

[source](https://developer.arm.com/products/architecture/cpu-architecture/m-profile/docs/100699/latest/memory-type-definitions-in-armv8-m-architecture)
