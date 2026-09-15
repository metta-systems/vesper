pub mod frame;
pub mod page_table;

// VSpace, ASIDPool, and ASID handlers remain excluded sketches: their kinds are
// not creatable (Retype allowlist) and their dispatch arms are inactive, so the
// draft files stay out of the build until their contracts are implemented.
