//! Arch-specific part of the TCB

struct UserContext {
    registers: [u64; 32],
}

pub(crate) struct TCB {
    register_context: UserContext,
}

pub(crate) fn user_stack_trace(thread: &TCB) {}
