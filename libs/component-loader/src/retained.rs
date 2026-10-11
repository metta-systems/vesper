//! The retained init image and low execution stack.

use {
    libobject::{KeySlot, RawKey, Rights},
    nucleus::{api::key_entry::KeyEntry, objects::KeyTable},
};

/// Accounted linked init image and retained low `EL1t` execution stack.
///
/// These occupied boot extents are not allocation sources: Retype would zero
/// live code/data/stack. The bootstrap builder may issue content-preserving
/// Frame authority over their pages instead. They remain reserved in `BOOT_INFO`
/// and must never be reclaimed while either execution or a mapping survives.
pub struct RetainedInitMemory {
    image: (u64, u64),
    stack: (u64, u64),
}

impl RetainedInitMemory {
    /// The retained extents, as `(start, end)` physical/identity-virtual
    /// bounds. Kickstart checks them against its boot accounting first.
    pub const fn new(image: (u64, u64), stack: (u64, u64)) -> Self {
        Self { image, stack }
    }

    /// Page-aligned physical/identity-virtual bounds of the entire linked image,
    /// including text, rodata (and embedded dependencies), data and BSS.
    pub fn image(&self) -> (u64, u64) {
        self.image
    }

    /// Page-aligned bounds of the reserved low `SP_EL0` execution stack.
    pub fn stack(&self) -> (u64, u64) {
        self.stack
    }

    /// Issue one bootstrap-origin Frame grant without initializing its contents.
    ///
    /// The one-time builder issues each retained page once, then uses checked
    /// `CopyDerive` for peers. This is boot authority, not a runtime creation ABI;
    /// it neither creates an `Untyped` over occupied bytes nor advances an
    /// unrelated allocation watermark. Mapping and derivation remain separate.
    pub fn grant_page(
        &self,
        table: &mut KeyTable,
        slot: KeySlot,
        guard: u32,
        paddr: u64,
    ) -> RawKey {
        assert_eq!(paddr & 4095, 0, "retained Frame must be page-aligned");
        let end = paddr
            .checked_add(4096)
            .expect("retained Frame extent overflow");
        assert!(
            [self.image, self.stack]
                .iter()
                .any(|&(start, limit)| start <= paddr && end <= limit),
            "Frame grant must lie wholly inside retained init backing"
        );
        table
            .insert(
                slot,
                KeyEntry::new_frame(paddr, 12, false, Rights::all()),
                guard,
            )
            .unwrap_or_else(|failure| {
                panic!("retained Frame grant failed: {:?}", failure.error.code())
            })
    }
}
