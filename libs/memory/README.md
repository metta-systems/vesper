# libmemory

libmemory contains only types (structs and traits) and abstractions useful for memory translation.
It includes memory mapping tables layout and MMU interface.

## Exports

Page table types must represent pages of differing sizes.
For every entry in the MMU page table we should be able to receive a proper page type - e.g. Invalid, further page table, or a specific-size page.

This library should export the following:

- MMU control interface under `interface::MMU`
- Page-table hierarchy representation. This needs to be platform-independent.
  Abstract translation stages and page size granularity.

   +---------------- work with MMU structures and kernel knowledge of mappings (THIS LIBRARY)
   |          +----- work with higher level mapping abstractions, without MMU details?
   v          v
libmmu -> libmapping

---

For more information please re-read.
