Key Design Points:

Aspect         | Design Choice              | Rationale
---------------+----------------------------+-----------------------------------------------
Type numbering | Core 0-15, Arch 16-63      | Clear separation, room for growth
Arch trait     | Associated types           | Compile-time type safety per arch
Pools          | Separate core + arch       | Different lifecycles, easier to reason about
Frame sizes    | Enum, not raw bits         | Type-safe, arch-specific validation
VSpace         | Wraps root PT + ASID       | Clean abstraction for address space
Dispatch       | Single match on ObjectType | Uniform handling, arch types get own handlers


Kernel API surface:

Wait options (open wait, like servers waiting; closed wait, like waiting one client's response specifically)
Timeouts for send and recv phases separately

Structure:

Key - a capability index in the keytable - this key is per-domain because it indexes a local domain table
KeyEntry - a capability "value" - entry in the keytable, capabilities are same-size values carrying type,
           pointer to kernel object and access rights.
A kernel object corresponding to the capability is allocated in the object pool.
Untyped does not have an "object", it points to memory region to allocate from, and amount of allocations already done.
