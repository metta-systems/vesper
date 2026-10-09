pub mod address_space;
pub mod asid_pool;
pub mod frame;
pub mod page_table;

// ASIDControl and I/O/IRQ handlers remain excluded sketches: their kinds are
// not creatable (Retype allowlist) and their dispatch arms are inactive, so
// no draft handler is built until their contracts are implemented.
// ASIDPool is dispatched through its API handler above (boot-provided
// capability-protected resource, selected 2026-09-15); the registered
// ASIDControl kind stays reserved with no pool or handler.
