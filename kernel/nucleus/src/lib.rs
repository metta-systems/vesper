//! Vesper nanokernel privileged nucleus — library surface.
//!
//! The nucleus binary (`main.rs`) is kept inert at boot: it performs no
//! initialization. Kickstart (the one-time boot code) constructs the initial
//! [`objects::Nucleus`] value from this library's types, writes it into
//! Untyped-carved memory, and records the anchor the binary reads on entry.
//! Sharing the exact types here guarantees the in-memory layout Kickstart
//! writes matches what the nucleus interprets.

#![no_std]
#![feature(allocator_api)]
#![feature(core_intrinsics)]
#![feature(decl_macro)]
#![feature(format_args_nl)]
#![feature(likely_unlikely)]
#![feature(ptr_internals)]
#![feature(slice_ptr_get)]
#![feature(stmt_expr_attributes)]
#![deny(warnings)]
#![allow(unused)]
#![allow(internal_features)]
// The `#[expect(…)]` annotations below were written for the bin's private-module
// context, where those lints fire. Exposing the same items as a public lib
// changes which lints fire, so the expectations are informational here; the bin
// keeps strict enforcement.
#![allow(unfulfilled_lint_expectations)]
// These lints only fire on public items. The lib exists to share kernel-internal
// types with the boot code, not as a polished public API; the bin keeps strict
// enforcement on its own surface.
#![allow(clippy::len_without_is_empty)]
#![allow(clippy::new_without_default)]
#![allow(clippy::pub_underscore_fields)]
#![allow(clippy::result_unit_err)]
#![allow(clippy::return_self_not_must_use)]
// These are kernel-internal types shared with the boot code, not a public API;
// the binary keeps `deny(missing_docs)` for its own surface.
#![allow(missing_docs)]

/// Syscall API — capability invocation handlers.
pub mod api;
/// Nucleus object implementations.
pub mod objects;

// Root-level re-exports the object modules reference via `crate::…`; the bin
// previously supplied these through its own `use` statements.
pub use objects::Nucleus;
