// ─────────────────────────────────────────────────────────────────
// Frame capabilities are now inline in KeyEntry as RegionPayload.
// No separate AArch64Frame struct needed — the capability IS the object.
//
// See: api/key_entry.rs (RegionPayload, KeyEntry::new_frame)
//      objects/untyped.rs (retype creates inline Frame caps)
//      api/arch/frame.rs (frame invocation handler)
// ─────────────────────────────────────────────────────────────────
