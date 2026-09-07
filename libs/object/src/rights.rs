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
        Rights(0xF)
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
