// Trait-like wrapper around kernel objects

impl KeyEntry {
    pub fn as_debug_console() -> Result<DebugConsole, SyscallError> {}
}
