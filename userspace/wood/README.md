A Doom port based on [Room](./room/README.md).

Version for vesper (should have clocks, render and demo runner):

- link in libroom and then
- setup a vesper app component from here
- wire up compositor, events,
- load WAD from resource
- no sound yet

requires:
- memory allocator ([rlsf](https://github.com/yvt/rlsf/tree/main/crates/rlsf))
- memio implementation to read from WAD resource

Phase 1:
- allocate app memory +16Mb
- init rlsf allocator there
- declare it as `#[global_allocator]` - ? needs a custom wrapper most probably
- create compositor window
- load WAD from `[[resource]]` with memio
- play autodemo

Phase 2:
- connect input-event stream from kb and mouse
- work with kb
- then work with mouselook too

Phase 3:
- PWM DMA audio
- Soundfont MIDI player
- sound mixer (midi rendered stream + effects)
- Isochronous playback thread

? Try Hexen WAD.

---

## Unsafe removal

Possible follow-ups:
- Profile that speed-up.
- Turn menu page pointers into indexes.
- Use slices in SHA1/checksum.
- Load the status bar and intermission graphics through typed handles instead of the unsafe callback tables.
- Move the renderer's texture, flat and colormap tables into owned Vecs. That would free G_InitNew/G_DoLoadLevel.
- Make sound channels a Vec.
- Make the demo buffers a Vec with explicit overflow emulation.
- Replace the module-level lint allows with commented blocks one module at a time.
- Look at R_MapPlane and finecosine in plane.rs: they're safe functions whose docs still describe real-sounding preconditions. That's a possible soundness gap.
