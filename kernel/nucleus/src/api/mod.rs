// TODO: move these api decls/impls to libraries in libs/, re-export only user part through
// TODO: libsyscall or something like that - usable from userspace.

mod buffer;
mod debug_console;
mod domain;
mod endpoint;
mod event_count;
mod key;
mod key_table;
mod notification;
mod reply;
mod syscall;
mod time;
mod untyped;

pub use syscall::{
    protected_call0, protected_call1, protected_call2, protected_call3, protected_call4,
    protected_call5, protected_call6,
};
