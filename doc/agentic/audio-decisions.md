# Audio open decisions

Parking lot for audio decisions the maintainer has not answered. Each entry stays open
until the maintainer answers it. Recommendations are marked as such; they are not
decisions. Record an answer only when the maintainer gave it explicitly; answered items
move to [audio-design.md](audio-design.md), and tasks stay in
[audio-implementation-plan.md](audio-implementation-plan.md).

Facts behind these questions (pins, registers, QEMU support) are in the design doc, with
sources.

D1 to D8 are answered (2026-10-27, the recommended option of each) and recorded in
[audio-design.md](audio-design.md), section Decisions. What remains:

## D9. Music synthesis (blocks A5)

- **A. Port the SoundFont path** (`rustysynth` or a no_std rewrite) with an SF2 file as
  a Kickstart `[[resource]]`. Matches the host; the SoundFont's size goes into the image
  and the synthesis cost is the largest of the plan.
- **B. An OPL2/OPL3 emulator** playing Doom's GENMIDI instruments, as the original game
  did. Small data, cheaper; a different sound from the host build.
- **C. No music on Vesper for now.**

Recommendation: decide after A4, with the CPU headroom measured; leaning A at 22 050 Hz
if it fits.

Answer (2026-10-27): decided after the sound effects (A4) work, with the CPU headroom
measured. The choice among A, B and C stays open until then.
