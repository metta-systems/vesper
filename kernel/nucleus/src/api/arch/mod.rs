pub mod asid_pool;
pub mod frame;
pub mod page_table;

// VSpace and ASID handlers remain excluded sketches: their kinds are not
// creatable (Retype allowlist) and their dispatch arms are inactive, so the
// draft VSpace file stays out of the build until its contract is implemented.
// ASIDPool is dispatched through its API handler above (boot-provided
// capability-protected resource, selected 2026-09-15); the registered ASID
// kind stays reserved with no pool or handler.
