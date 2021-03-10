# Memory Configuration

The types VirtAddr and PhysAddr are representing the addresses before and after the mapping in the MMU.

Page table types must represent pages of differing sizes.
For every entry in the MMU page table we should be able to receive a proper page type - e.g. Invalid, further page table, or
a specific-size page.

----

For more information please re-read.











----

## Plan

1. MMU tables - because we need separate memspaces for kernel and userspace
   1a. Allocate initial page tables
   1b. Map over available RAM sensibly
   1c. Create kernel's own mapping (TTBR_EL1)

## What does the kernel MMU code support?

* mapping
* unmapping
* switching per-process mappings (virtspaces)
* virt2phys resolution
* direct phys access for kernel (TTBR_EL1 mapping to physmem)
* initial kernel memory allocation: for mapping tables and capnodes, for initial thread TCB and stacks

## public api

    ARMMU invocations:
        on page directory cap
            cache maintenance (clean/invalidate/unify)
        on page table cap
            map
            unmap
        on small frame/frame caps
            map
            remap
            unmap
            cache maintenance (clean/invalidate/unify)
            get address
        on asid control cap
        on asid pool cap


## Minimum Required Functionality (build from this)

* resolve VA to PA - resolving lets kernel access mapped process memory.
  (start from the process' virtspace root - Page Directory)
* flush page, pd, pt, virtspace - will be important for thread switching
* map a page table to appropriate location
* unmap entire mapped page table
* map a phys frame to virt location
* unmap a mapped frame


## Requirements

GIVEN
    A PageGlobalDirectory of process
FIND
    A kernel-physical address of where it contains a certain leaf node.

## sel4

> seL4 does not provide virtual memory management, beyond kernel primitives for manipulating hardware paging structures. User-level must provide services for creating intermediate paging structures, mapping and unmapping pages.
> Users are free to define their own address space layout with one restriction: the seL4 kernel claims the high part of the virtual memory range. On most 32-bit platforms, this is 0xe0000000 and above. This variable is set per platform, and can be found by finding the kernelBase variable in the seL4 source.
(from https://docs.sel4.systems/Tutorials/mapping.html)

> Note that to map a frame multiple times, one must make copies of the frame capability: each frame capability can only track one mapping.

## howto steps

initial mapping:
* for kernel space - 
    1. obtain memory map
    2. build a list of regions available as RAM
    3. find largest covering page sizes
    4. allocate several memory pages and fill them with table info
       (need page table creation functions here)
    5. now kernel is able to address physical memory through it's (negative) kernel mapping.
    6. prepare init thread VSpace
        - this is more complicated wrt mapping
