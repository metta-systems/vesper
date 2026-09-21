#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rights(pub u8);

impl Rights {
    pub const READ: u8 = 0x1;
    pub const WRITE: u8 = 0x2;
    pub const MAP: u8 = 0x2;
    pub const SEND: u8 = 0x2;
    pub const RECV: u8 = 0x1;
    pub const CALL: u8 = 0x4;
    pub const GRANT: u8 = 0x8;
    /// Execute permission for frame mappings (selected 2026-09-15): a
    /// `Frame.Map` requested with `EXECUTE` — within the frame capability's
    /// rights — clears PXN|UXN for the installed descriptor. Interim AP
    /// semantics: with `WRITE` the mapping is kernel-privilege RW+X (EL0
    /// denied — EL1 cannot execute EL0-writable pages); without `WRITE` it is
    /// read-only executable at EL0 and EL1. Splitting user/privileged
    /// execute is future work with EL0 entry (D6).
    pub const EXECUTE: u8 = 0x10;

    /// Thread lifecycle control: a `Thread.Retire`
    /// invocation requires `RETIRE` on the invoked Thread capability.
    /// Delegable like other capability permissions — retirement authority
    /// follows capability permissions, not a privileged owner identity.
    /// Carving a Thread from Untyped stays unrepresentable (Retype rejects
    /// the kind), so bootstrap grants are the initial source of this right.
    pub const RETIRE: u8 = 0x20;

    // ── KeyTable-management permissions (per-kind interpretation) ──
    // Selected 2026-09-07 (D4): KeyTable capabilities interpret the rights
    // field as table-management permissions, reusing the same bit positions
    // as other kinds (per-kind bit reuse is permitted by the contract).
    /// Source derivation: CopyDerive/Move from entries in this table.
    pub const DERIVE: u8 = 0x1;
    /// Source removal: Move-out and Delete of entries in this table.
    pub const REMOVE: u8 = 0x2;
    /// Destination installation: CopyDerive/Move into this table.
    pub const INSTALL: u8 = 0x4;
    // Bit 3 (0x8) is reserved for future table administration and is not
    // granted initially.

    pub const fn empty() -> Rights {
        Rights(0)
    }
    pub const fn all() -> Rights {
        Rights(0x3F)
    }
    pub fn bits(&self) -> u8 {
        self.0
    }

    /// Whether `self` has every permission bit in `required`.
    pub fn has(&self, required: u8) -> bool {
        self.0 & required == required
    }

    /// Whether `requested` is a subset of `self` (no amplification).
    pub fn permits(&self, requested: Rights) -> bool {
        requested.0 & !self.0 == 0
    }
}
