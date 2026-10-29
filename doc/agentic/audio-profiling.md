# Audio profile: the music synthesiser and Wood's playback path

Measured 2026-10-29 on the host (x86_64), and in QEMU 11.1.2 (raspi3b, TCG) with
two small instruction-counting plugins. The board figures quoted are the
maintainer's (Pi 3 B, at 600 MHz, and at 1200 MHz with `force_turbo=1`). The
profile itself added only the `profile_render` benchmark and the
`just profile-synth` recipe. Proposal 4, the voice-loop rewrite, has since been
done; its before and after numbers are in [Proposal 4: done](#proposal-4-done).
Proposals 1 (as playback priority), 2, 3 and 5 followed the maintainer's answers
of the same day: see [Scheduling, start-up and mixer: done](#scheduling-start-up-and-mixer-done).
The sections before them describe the code as it was before those changes.

## Summary

- **The synthesiser is cheap enough.** Its voice loop costs **about 57 aarch64
  instructions per voice-frame**, or about 61 with the per-block envelope and ramp
  setup. The A53 runs roughly one instruction per cycle here, so a period costs
  about 100 ns per voice-frame at 600 MHz:
  - with 32 voices, a 512-frame period is about 1.6 ms of an 11.6 ms period
    (14 %) at 600 MHz, and 0.8 ms at 1200 MHz;
  - the board's `midi-test` maxima (about 1.5 ms with 31 voices at 600 MHz) agree.
- **Wood's 20–31 ms spikes are scheduling, not work.** In QEMU under `wood-test`
  the synthesiser executed **at most 847 k instructions in any one service call**
  (p99 746 k). That is at most 1.4 ms at 600 MHz, yet the wall-clock service
  maxima in the same run reached 7–30 ms. The root dispatcher's round-robin
  explains it:
  - the slice is a fixed 10 ms of wall time;
  - any wakeup preempts the running Thread, and the preempted Thread goes to the
    back of the round;
  - three CPU-bound peers (Wood's engine Thread, the compositor and the lava lamp)
    make one preemption cost 2–3 slices, that is 20–30 ms.

  This holds whatever the ARM clock, which matches the board: 1200 MHz did not
  shrink the maxima.
- **The startup underrun has a specific cause.** Wood opens the audio output
  before queueing anything. It then wakes the engine (`answer`) before its first
  refills, so the stream can start counting underruns with only one buffer queued,
  while the engine runs `R_Init` in 10 ms slices beside the compositor and its
  USB start-up.
- **The voice loop is now more than twice as fast, with no hash changed**
  (proposal 4, done; see [Proposal 4: done](#proposal-4-done)). It went from
  55.7 to 21.6 instructions per voice-frame, and the synthesiser from 45.7 M to
  25.1 M instructions on `midi-test`. Before, 39 % of the loop was the sample
  fetch (bounds checks, and byte loads forced by `+strict-align`), and a further
  26 % the loop-mode `match`, repeated three times per frame. This was worth
  doing, but it is not what threatens underruns.

## Method

### 1. Host benchmark: `just profile-synth`

`profile_render` is an `#[ignore]` test in `userspace/wood/synth/tests/doom_music.rs`.

- It renders each of the 13 shareware songs for 60 s at `OUTPUT_RATE`, in 512-frame
  periods, as the playback Thread does.
- It uses the same trimmed SC-55 bank that `just wood-soundfont` writes, built in
  memory from `userspace/wood/wad/doom1.wad` and the SC-55 file, as the golden-hash
  tests do.
- It prints:
  - ns per frame;
  - ns per voice-frame, with voices counted at the start of each period;
  - the mean and maximum voices;
  - the worst single period.

```sh
just profile-synth
# = RUSTFLAGS="-D warnings -Z macro-backtrace" cargo test --release -p wood-synth \
#     --features=host-tests --test doom_music -- --ignored --nocapture \
#     --test-threads=1 profile_render
```

`perf` is installed but unusable: `kernel.perf_event_paranoid` is 4 and changing
it needs sudo. Valgrind is not installed. So the function breakdown below comes
from the aarch64 instruction counts, not from host sampling.

### 2. aarch64 instruction counts: QEMU TCG plugins

The linuxbrew QEMU 11.1.2 ships `qemu-plugin.h`, but no prebuilt plugins
(`libhotblocks.so` and the others are absent). I wrote two small plugins against
the header. They are throwaway, in `/tmp/profile-plugin/`, and built with the
system gcc and linuxbrew glib:

```sh
gcc -O2 -shared -fPIC -o libhotpc.so hotpc.c \
  -I/home/linuxbrew/.linuxbrew/opt/qemu/include \
  $(PKG_CONFIG_PATH=/home/linuxbrew/.linuxbrew/lib/pkgconfig pkg-config --cflags glib-2.0)
```

- **`hotpc`** counts executions of every translation block, keyed by (vaddr, FNV
  hash of the block's code bytes), and writes the counts at exit.
  - Every component is linked at `0x400000`, so a vaddr alone is ambiguous.
  - `attribute.py` therefore matches each block's bytes against the `.text` of
    each candidate ELF in `target/rpi3-qemu+debug_kernel/aarch64-metta-none-eabi/release/`
    (`rust-objcopy --only-section=.text`), then maps it to a symbol with `rust-nm`.
- **`percall`** counts only the instructions in blocks whose bytes match one
  component's `.text`.
  - It records the running count each time the block at `entry=` executes. Here
    that is `midi_synth_v1_service` at `0x4001f8`.
  - Each record is therefore the synthesiser's own instructions per service call,
    whatever else ran in between.

Runs (logs in `target/profile-*.log`, never `qemu.log`; the QEMU processes were
stopped by `timeout -s TERM`, so the plugins' exit handlers ran):

```sh
just build-midi-test && just build-wood-test   # PATH=/tmp/wood-bin:$PATH if rust-objcopy is missing
qemu-system-aarch64 -M raspi3b -display none -monitor none \
  -chardev stdio,mux=on,id=char0,signal=off -serial file:target/profile-midi-serial.log \
  -semihosting-config enable=on,userspace=on,chardev=char0 -dtb target/audio-test.dtb \
  -kernel target/images/midi-test-rpi3-qemu+debug_kernel.bin \
  -plugin /tmp/profile-plugin/libhotpc.so,outfile=/tmp/profile-plugin/midi-hot.txt \
  < /dev/null > target/profile-midi.log
# wood-test: the same with -device usb-hub,port=1 -device usb-kbd,port=1.1, the wood-test
# image, `timeout -s TERM 150`, and either libhotpc.so or
# libpercall.so,text=/tmp/profile-plugin/midi-synth.text,base=400000,entry=4001f8,outfile=...
python3 -I attribute.py midi-hot.txt <release dir> midi-synth midi-player audio-pwm-bcm2835 ... nucleus
```

To read the inner loop, I disassembled it with `rust-objdump -d` on the release
`midi-synth`. `Voice::render` and `Synth::render` are inlined into
`__body_midi_synth_v1_service`; the loop is at `0x4022fc`–`0x402450`.

## Numbers

### Host (x86_64, release)

The host is noisy: the first songs of a run come out 1.5–2.5× slower, from clock
ramp-up and other load. The figures below are from a settled run:

| song | ns/frame | ns/voice-frame | mean voices | max voices |
|---|---:|---:|---:|---:|
| D_E1M1 | 43.5 | 4.4 (cold) | 9.9 | 31 |
| D_E1M6 | 57.3 | 2.35 | 24.4 | 32 |
| D_E1M9 | 42.4 | 2.44 | 17.4 | 32 |
| D_INTRO | 69.7 | 2.38 | 29.3 | 32 |
| D_INTER | 36.1 | 2.57 | 14.1 | 24 |
| all 13 | | 2.4–2.6 settled | | |

- Real-time needs 22.7 µs per frame, so the host renders about 300–1000 times
  faster than real time.
- The mean voices are high on several songs: 29.3 on D_INTRO and 24.4 on D_E1M6.
  Those counts include release tails that are still above `SILENT_CENTIBELS`;
  see proposal 7.

### aarch64: where the synthesiser's instructions go

This is the `midi-test` run: 512 periods of D_E1M1's opening, 2.15 voices on
average (562 k voice-frames), and 45.7 M synthesiser instructions in all.

| part | instructions | share | per unit |
|---|---:|---:|---|
| voice inner loop (`voice.rs` render loop) | 31.3 M | 68.6 % | **55.7 per voice-frame** |
| output scale and clip (`Synth::render` → `clip`) | 5.2 M | 11.4 % | 10 per sample |
| `memset` (block zeroing, the Session's stack chunks) | 2.2 M | 4.9 % | 4.4 k per period |
| per-voice per-block setup (bend, `start_ramp`, envelope) | 2.1 M | 4.7 % | ~3.8 per voice-frame (~240 per voice per 64-frame block) |
| `memcpy` (data page, voice copies) | 1.4 M | 3.1 % | 2.8 k per period |
| envelope functions outside the inline code | 0.4 M | 0.8 % | |
| sequencer, SMF reading, session, IPC glue | ~3 M | ~6.5 % | |

The fixed cost per period is about 24 k instructions. Each voice adds about
61 × 512 ≈ 31 k instructions per period. So at 15 voices the inner loop is about
90 % of the synthesiser.

#### The inner loop, per voice-frame (Continuous loop mode, no wrap)

Read from `rust-objdump`, and consistent with the measured 55.7:

| role | instructions | share |
|---|---:|---:|
| sample fetch: 2 × (range check, 2 `ldrb` + `lsl` + `sxth` + `orr`), the `next < end` check | 22 | 39 % |
| `looping()` evaluated 3 times (match on `mode`, `released`), next-index select, wrap check | 15 | 26 % |
| mix and pan: `ldp`, 2 × `smull`, 2 × `add`, `stp` | 6 | 11 % |
| loop control (frame count, output pointer) | 5 | 9 % |
| interpolation (`sub`, `lsr`, `mul`, `add asr`) | 4 | 7 % |
| position (`lsr` index, `add`, `str` to the voice) | 3 | 5 % |
| gain ramp (2 `add`) | 2 | 4 % |

Observations from the generated code:

- **No divisions and no calls in the loop.** The i64 products are single `smull`
  instructions, and the 32.32 position is one 64-bit `add`.
- **The byte loads are real cost.** `SoundFont::sample_data` is `&[u8]`, and
  `targets/aarch64-metta-none-eabi.json` has `+strict-align`. So each point
  becomes two `ldrb`, a shift, a sign-extend and an or, plus the slice range check
  of `get(index*2..index*2+2)`.
- **`self.looping()` is not hoisted.** `mode` and `released` are reloaded into
  registers, but the `match` branches run three times per frame.
- **The position is stored to the `Voice` every frame** (`str x0, [x27, #8]`).
- **The output scale and clip loop** is branch-free (`csel`), at 10 instructions
  per sample.
- **Nothing needs SIMD.** NEON would help the mix-and-pan and the clip loops
  (2 lanes), but FP/SIMD is trapped and the build is soft-float, so this is noted
  only.

#### Cycles

The A53 is in-order and dual-issue. With a multiply chain and two dependent loads
per point, I estimate 0.8–1.2 instructions per cycle, that is 50–75 cycles per
voice-frame. The board agrees:

- the `midi-test` maximum of about 1.5 ms at 600 MHz with up to 31 voices is
  0.9 M cycles;
- the model predicts 31 × 512 × 61 + 24 k ≈ 0.99 M instructions.

| | per voice-frame | 32 voices, one period | share of the 11.6 ms period |
|---|---:|---:|---:|
| 600 MHz | ~100 ns | ~1.65 ms | 14 % |
| 1200 MHz | ~50 ns | ~0.82 ms | 7 % |

### The rest of the per-period path (aarch64 instructions per 512-frame period)

| stage | `midi-test` | `wood-test` | notes |
|---|---:|---:|---|
| synthesiser `service` | 89 k (2 voices) | 410 k mean, 847 k max | from `percall`, 10 291 calls |
| client copy and level (midi-player `refill`) | 20 k | — | about 31 per frame |
| Wood `Output::refill` (music copy, audible scan, `mix_shared_onto`) | — | 92 k | see below |
| audio driver (`Stream::take`, conversion, copy) | 21 k | 20 k | about 40 per frame: `word()` multiplies, 4 KiB to the ring |
| DMA driver | 6 k | 6 k | |
| root dispatcher | 4 k | — | |

Wood's `refill` is dominated by `mix_shared_onto`:

- a 4-instruction check of each of the 8 effect channels for every frame:
  4.1 k checks per period, about 16 k instructions;
- about 18 instructions per active effect voice per frame;
- 25 instructions per frame for the output.

There is no allocation per period in steady state, and no logging inside the
measured service window except:

- the synthesiser's `note()` lines on song events;
- Wood's `report()` at power-of-two audible counts, after the timing.

Each of those lines wakes the log server, which preempts the caller.

#### What `wood-test` runs (QEMU, 150 s, 155 G guest instructions)

| component | share |
|---|---:|
| wood | 56.2 % (`I_FinishUpdate` 29.2 %, `draw_frame` 20.0 %, the 3D renderer ~5 %) |
| compositor | 23.7 % |
| draw-demo (the lava lamp) | 9.6 % |
| nucleus | 6.3 % |
| midi-synth | 3.9 % |
| everything else | < 0.5 % |

The CPU is saturated by three peers that always have work. As a side finding,
Wood spends ten times more on presenting a frame than on rendering it.

## Wood's spikes: verdict

**Preemption, not work.** The evidence:

1. **Work against wall time.** The `percall` plugin in QEMU `wood-test`:

   | measure | value |
   |---|---:|
   | instructions per call, mean | 410 k |
   | p50 | 418 k |
   | p99 | 746 k |
   | p99.9 | 793 k |
   | max | 847 k |
   | wall-clock service max per 256 periods, same run | 4.9–29.9 ms |
   | wall-clock refill max at startup, same run | 61 ms |

   Even at QEMU's speed, 847 k instructions take about 2–3 ms. On the board that
   is ≤ 1.4 ms at 600 MHz, and ≤ 0.7 ms at 1200 MHz.
2. **The same code, the same measurement, without the peers.** `midi-test` has no
   compositor, lava lamp, USB or engine. Its steady maxima are 0.8 ms in QEMU and
   about 1.5 ms on the board.
3. **The clock does not matter.** At 1200 MHz the board showed 30 ms at startup
   and 20–23 ms in steady play, the same as at 600 MHz.
4. **The mechanism, from the code.** The agent that read the code verified each
   point:
   - **The clock is wall time.** Both measures use `libuser::clock::now_ns()`
     (`cntvct_el0`, `userspace/runtime/src/clock.rs`). There is no per-Thread CPU
     time.
   - **The synthesiser runs on Wood's playback Thread.** `midi_synth_v1` is a
     local interface: `service` runs on the caller's Thread, so any preemption
     lands inside the window.
   - **The slice is fixed in time.** The root dispatcher has
     `SLICE_NS = 10_000_000` (`userspace/dispatcher/src/main.rs:28`): 10 ms of
     wall time, not cycles.
   - **There are no priorities and no budget limits.** Every driver's budget is
     `u64::MAX`, and `wood-test/image.toml` sets none. The kernel runs on one CPU.
   - **Every wakeup preempts.** The dispatcher calls
     `configure_activation(ALL)`, and `make_runnable` raises an activation for
     every wakeup.
   - **The search restarts after the preempted slot**
     (`next = slot % slot_count + 1`). The preempted Thread is the last candidate,
     and the woken Thread gets no preference either.
   - **Wakeups are frequent.** They come from the engine's 1 ms `I_Sleep`, the
     compositor's 40 ms frame timer, the lava lamp, the USB IRQs, the DMA, and the
     log server on any log line.

   After a preemption, the playback Thread waits until the engine, the compositor
   and the lava lamp have each run up to 10 ms. That is 20–30 ms whatever the
   clock, which is exactly what the board shows. The same delay also applies to
   *waking* the playback Thread, and that part is not in the refill figure, so
   the real slack consumed is larger than the logged maxima.

What was not separated directly:

- The kernel keeps `Thread::run_time_ns`, but only the root dispatcher can read
  the record table, and I did not instrument it.
- A decisive cheap check on the board would be to lower `SLICE_NS` to 2–4 ms and
  see whether the maxima scale down with it.

### Startup (coordinator's second board run: 30.6 ms, 1 underrun at 1200 MHz)

- **Why startup is worse.** The engine is not uninterrupted: the dispatcher
  preempts it every 10 ms. But during `R_Init` and the asset loading it stays
  runnable for seconds. Meanwhile:
  - the compositor creates windows;
  - USB enumerates;
  - the log is busiest, and each line is a log-server wake, so another trip to the
    back of the round for the playback Thread.

  In QEMU, the first summary of the second `wood-test` run had a refill maximum of
  **61 ms**, against 7–29 ms later.
- **Why an underrun is possible at startup.**
  - `playback()` calls `Output::open` (`client.open()`, which starts the ring with
    a ramp-up), then `Music::open`, then `answer(&hand_over)`, and only then
    `play()`, which queues the first 4 buffers.
  - `answer` wakes the engine, and that wakeup preempts the playback Thread before
    its first refill.
  - The driver counts underruns from the first packet. If the playback Thread is
    preempted after one packet (by the title song's upload, whose `service`
    chunks log through `note()`, or by the next wake), the stream starts with
    11.6 ms queued plus the ring instead of 46 ms plus the ring.
  - `midi-player` avoids this: it queues 4 buffers before `open`.

Ranked mitigations for the startup underrun:

1. **Pre-fill before `open`**, as `midi-player` does: refill 0..4, then
   `client.open()`, then `answer()`.
   - Gain: the stream starts with all 4 buffers, which closes the startup window
     completely.
   - Effort: about 10 lines in `sound.rs` (`Music::open` moves before the
     pre-fill).
   - No latency cost, and no hash change: the synthesiser's start hash is
     computed in the component and does not depend on when `open` happens.
   - No decision needed.
2. **More queued buffers.**
   - `QUEUED_BUFFERS = 8` fits the existing 4 data pages; `channel 4;` holds
     8 × 2 KiB.
   - It doubles Wood's slack to 93 ms plus the ring's 46 ms, which covers a
     3-slice stall with margin.
   - It costs 46 ms more effect latency (about 100 → 150 ms).
   - 6 buffers (70 ms) is a middle ground.
   - Needs: the maintainer's latency trade.
3. **Playback-Thread priority, or fixing the round-robin's treatment of
   preemption.**
   - This is the root cause, and it also fixes steady-state jitter.
   - Options:
     - (a) after an activation, resume the woken or preempted Thread first, by
       not advancing `next` past a preempted Thread;
     - (b) a shorter `SLICE_NS`;
     - (c) a favoured slot or a priority flag in the root dispatcher.
   - All of these live in userspace (the root dispatcher), so no new kernel
     API is needed.
   - Needs: a decision. D8 said "no special scheduling first"; the data now
     shows 2–3-slice stalls.
4. **Delaying the music until the engine has started.**
   - This only takes the song upload out of the startup window. The silent
     buffers still start underrun counting, and the engine still competes.
   - It is the weakest of the four.

## Optimisation proposals, ranked

"Hash" means the golden hashes (`golden_renders`, the midi-synth session start
hashes, `midi_test_hash`, `wood_music_hashes`).

| # | proposal | expected gain | effort | risk | hash | decision |
|---|---|---|---|---|---|---|
| 1 | **Scheduling: don't send a preempted Thread to the back of the round** (dispatcher `next` handling), and/or a **shorter slice** (10 → 2–4 ms), and/or **playback priority** in the root dispatcher | Removes the 20–31 ms stalls, the only real underrun risk. Stall bound becomes about one slice of the woken peer, or about 0 with priority | small (a constant, or about 20 lines in `userspace/dispatcher`) | medium: dispatch overhead grows about 10/slice ×; fairness changes | no | yes: revisits D8 |
| 2 | **Wood: pre-fill before `open`**, then `answer` | Removes the startup underrun window | small | low | no | no |
| 3 | **Deeper Wood queue**, `QUEUED_BUFFERS` 4 → 6 or 8 | Slack 46 → 70 or 93 ms | trivial (8 fits today's pages; beyond that, `channel N;` in `audio_v1.if`) | low; +23/+46 ms effect latency | no | yes: the latency |
| 4 | **Done 2026-10-29** (55.7 → 21.6 per voice-frame, hashes unchanged; see below). **Voice loop rewrite, same arithmetic.** (a) A `render` per loop state: match `mode` and `released` once per call, not 3 × per frame. (b) Read the bank's samples as an aligned `&[i16]`, checked once at `SoundFont::parse`, through a safe `as_chunks`/`align_to` check, or one `unsafe` cast with the alignment check as its proof. (c) Split each call into runs that cannot reach `end`, `loop_end` or the wrap, check the indices once per run, and drop the per-frame bounds and wrap checks. (d) Keep `position` in a local | Inner loop 57 → about 25–30 instructions per voice-frame: about 2 × on the synthesiser, so 32 voices ≈ 0.8 ms at 600 MHz | medium | low–medium: the loop-wrap edge cases are covered by the `synth` host tests and the render-split test | **no**, if bit-exact; the golden hashes prove it | no |
| 5 | **Wood `mix_shared_onto` channel-major.** Loop over active channels outside the frame loop, and skip the mix when none plays | 92 k → about 30 k instructions per period (about 0.1 ms at 600 MHz) | small | low | no; same sums, provided the per-sample clip happens after all channels, as now | no |
| 6 | **Log off the hot path.** Defer the synthesiser's `note()` lines and Wood's `report()` out of `service` and `refill`, for example to the summary | Removes self-inflicted preemptions while 1 is undecided | small | low | no | no |
| 7 | **End inaudible tails early.** Finish a released voice when its gain in Q16 falls below about 4 (≈ −84 dB), instead of at 96 dB | Unknown, possibly large on tail-heavy songs (mean voices 24–29 on D_INTRO and D_E1M6). Measure first: add a count of voices under the threshold to `profile_render` | small | low audibly (≤ 1 LSB) | **yes** | yes: an output change |
| 8 | **Raise the ARM clock** (`force_turbo=1`, or mailbox `set_clock_rate`) | Halves every CPU cost, the synthesiser's included, but **does not fix the stalls** (the board shows it) | trivial (config.txt) or small (mailbox) | thermal and power on the Pi 3 B | no | yes |
| 9 | **Fold the master volume into the voice gains, or clip faster** | 10 instructions per sample, so about 10 k per period (≈ 1 voice's worth) | small | low | yes: the rounding changes | no, but it costs a hash update |
| 10 | **Coarser envelope blocks** (64 → 128 frames) | About 2 instructions per voice-frame (3 %) | trivial | slightly coarser ramps | yes | yes |
| 11 | **Polyphony 32 → 24** | Caps the worst period at about 1.2 ms at 600 MHz | trivial | voices stolen sooner in dense passages | only when more than 24 voices sound | yes |
| 12 | **Half-rate render with an upsampler** | About 2 × on the voice loop, minus the upsampler (about 10 instructions per frame) | medium | quality; aliasing | yes | **yes: reverses D9.5** |

Evaluated and **not** recommended:

- **16.16 or 32-bit positions instead of 32.32.** On aarch64 the 64-bit position is
  one `add`, and the fraction is one `lsr`. Nothing to gain.
- **i32 instead of i64 products.** `smull` is a single instruction; the i64
  arithmetic is already free.
- **`get_unchecked`.** Proposal 4(c) removes the checks in safe code, by checking
  once per run.
- **Per-block gain instead of the per-frame ramp.** It saves 2 instructions per
  voice-frame, but brings back the clicks the ramp exists to prevent.
- **Skipping finished voices.** This is already done: `render` returns at once,
  the slot is freed, and empty slots cost one `Option` check each. Only proposal 7
  (inaudible tails) is left.
- **Precomputing per-zone values.** Zone lookup and `Voice::new` do not show in
  the profile: `generator_at`, `bag_at` and `scale_by_cents` together are < 0.3 %
  of the synthesiser.

## Proposal 4: done

Done 2026-10-29. The output is bit-exact: no hash changed.

What changed:

- **Samples as `&[i16]`.** `SoundFont::parse` checks once that the `smpl` chunk
  starts on a 2-byte boundary (and that the target is little-endian), and keeps
  the points as `SoundFont::points: Option<&[i16]>`. That is one `unsafe`
  `align_to::<i16>`, accepted only when both its prefix and suffix are empty;
  `i16` has no invalid bit patterns. A bank at an odd address gets `None` and
  renders through the old per-frame path; `unaligned_bank_renders_the_same`
  (in `doom_music.rs`) checks that both paths give the same output.
- **Runs.** `Voice::render` reads the loop state once per call. It then
  alternates:
  - a run of frames that cannot reach `loop_end` (looping) or `end` (not
    looping): its length is one division, so the frames need no end, wrap or
    `next` check;
  - one frame the old way, at the loop's or the sample's end.

  A run checks its highest index once (`&points[..last_index + 2]`), and each
  frame clamps its index to it (`cmp` and `csel`), so there is no panic branch in
  the loop. The wrap happens once, after the run's last step.
- **Locals.** The position and the gains stay in registers for the whole call.
- **Output stage.** `Synth::render` and `mix` zero the block as they read it
  (`mem::take`), instead of `fill(0)` before each block. The clip loop works one
  stereo frame per iteration, and the master volume is an `i32`, so its scaling
  is one `smull`.

The run's loop is 21 instructions, with no branch other than the loop's own:

```
lsr, cmp, csel           index, clamped to the run's last index
lsr                      fraction
add, add                 gain ramp
add (index*2), add       address, position
ldrsh, ldrsh             the two points
sub, mul, add asr        interpolation
ldp, smull, smull, add, add, stp   mix and pan
cmp, b.ne                loop
```

### Numbers

These are aarch64 instruction counts from the `hotpc` plugin on `midi-test`.
The workload is the same and deterministic: 512 periods of D_E1M1, with
562 k voice-frames.

| | before | after |
|---|---:|---:|
| voice loop (runs, slow frames, wraps, run setup) | 31.3 M | 12.1 M |
| per voice-frame | **55.7** | **21.6** |
| of which in runs (the 21-instruction loop) | — | 11.6 M (98 % of voice-frames) |
| output scale and clip, with the block zeroing | 5.2 M + block `memset` | 5.0 M |
| `memset` (the rest: the Session's stack chunks) | 2.2 M | 1.05 M |
| `memcpy` (unchanged; in the component) | 1.4 M | 1.4 M |
| synthesiser total | 45.7 M | 25.1 M (−45 %) |

- With the per-block setup, a voice-frame now costs about 25 instructions
  instead of 61.
- At 600 MHz, 32 voices over a 512-frame period come to about 0.7 ms instead of
  1.65 ms.
- The host barely moves: x86_64 has no `strict-align`, and its code was already
  close. A settled `just profile-synth` gives 2.35–2.38 ns per voice-frame on
  the busy songs, against 2.38–2.57 before. The all-song means were 2.5–2.6
  before and 2.4–2.6 after, which is within the host's noise.

### Proof

- **Hashes unchanged:**
  - `golden_renders` (D_E1M1 `0xcdda6c33f05c3732`, D_INTRO
    `0x74067a958e5dd608`);
  - the session's start hashes (D_INTROA `0x40f16e7eae7a5f45`, D_E1M5
    `0x4f0a9ddf3a4a14be`, and the `midi-test` song `0x4366085f6c416218`);
  - `just test-midi` in QEMU, whose first 64 periods hash
    `0x4366085f6c416218`.
- **Checks:** `just test-wood-synth` and `just synth-no-float` pass. Clippy is
  clean on `wood-synth` (the target build and its host tests) and on the
  `midi-synth` session test. The known `metta-meddle` breakage still stops the
  target clippy of the component.

Not done: the `memcpy` (the component's data page and voice copies) is in
`midi-synth`, outside this change.

## Scheduling, start-up and mixer: done

Done 2026-10-29, after the maintainer's answers (recorded with D8 in
[audio-design.md](audio-design.md)):

- **Proposal 1, as playback priority.** "declare priority with the thread, two
  levels should be enough for now, keep room for extending in the future, no
  starvation guard for this round". The root dispatcher now runs the highest
  runnable priority level first and round-robins within a level; Wood's playback
  Thread is declared high (`driver_threads!(playback => Priority::HIGH)`). The
  slice stays 10 ms. The rule is in the
  [Time contract](capabilities-contract.md#time-and-userspace-scheduling).
- **Proposal 2.** "yep": Wood queues its first buffers, with the music, before
  `open`, then answers the game Thread.
- **Proposal 3.** "8 buffers might be good": Wood and midi-test's player keep 8
  buffers (93 ms) queued, the channel's four data pages.
- **Proposal 5.** "yes": `Mixer::mix_onto` is voice-major in runs of 64 frames and
  returns at once when no effect plays. `voice_major_mix_is_bit_exact` compares it
  with the frame-major mix; the music hashes of `test-wood` held.
- **Also:** draw-demo's lava lamp sleeps on the compositor's events while its
  window is unfocused, so in `wood-test` it no longer competes with Wood.

### Numbers (QEMU `wood-test`, the four music summaries of one run, in ms)

| run | `service` max | refill max | underruns |
|---|---|---|---:|
| before (round-robin, 4 buffers, open before filling) | 18.6, 18.1, 15.0, 15.5 | 18.7, 18.7, 20.7, 15.7 | 0 |
| everything below, but the playback Thread normal | 11.0, 7.4, 1.2, 9.0 | 15.1, 7.4, 9.2, 9.5 | 0 |
| after, run 1 | 3.7, 1.1, 0.41, 0.38 | 5.95, 1.23, 0.45, 0.44 | 0 |
| after, run 2 | 1.7, 0.65, 0.52, 0.50 | 3.2, 1.0, 0.70, 0.62 | 0 |

- The "before" run predates proposal 4 too, so its averages are higher for that
  reason as well; the control run (normal priority, everything else as after)
  isolates the priority: the stalls of up to a slice come back without it.
- The first summary of each run covers start-up (the engine's `R_Init`, USB
  enumeration, the busiest log), where the remaining few milliseconds are.
- `driver-test` checks the priority on its own: a high Thread sleeping 2 ms beside
  two spinning normal Threads wakes at most 0.7 ms late, and 10.4 ms late when
  declared normal.
- Not measured: the board. The next board run should show the steady-state
  maxima near the synthesiser's own cost (about 1 ms at 600 MHz).

## Wood's frame path: done

Done 2026-10-29, on the maintainer's instruction to implement all five
changes and measure the fifth. Before, `I_FinishUpdate` (29.6 %) and Wood's
`draw_frame` (20.3 %) took half of `wood-test`; the renderer took about 5 %.

What changed:

1. **The engine draws into the compositor's surface.** Room's `Platform`
   gained `draw_in_place(&mut dyn FnMut(&mut [u32], usize)) -> bool`: the
   platform calls the closure with its buffer and pitch, then presents. Wood
   lends its surface and commits after a `fence(Release)`; the per-pixel
   copy loop and its `write_volatile` are gone. The default returns `false`,
   and the engine keeps a buffer of its own for `draw_frame`, as before
   (the host and the demo test are unchanged; `DG_ScreenBuffer` is gone).
2. **Finished-pixel palette.** `Platform::finish_pixel(r, g, b) -> u32`
   (default `0x00RRGGBB`) is asked only in `I_SetPalette`, for 256 entries.
   Wood's answer has the display's channel order and `OPAQUE` folded in. The
   dead `-gfxmode rgb565` path is removed (nothing passed it; its 16-bit
   pixels never matched a 32-bit `draw_frame`).
3. **Aligned word stores.** `convert_frame` writes a `&mut [u32]`; the 2x
   loop is `ldrb`, `ldr w`, **`stp w10, w10`** per source pixel (7
   instructions per two pixels), where the old loop stored each pixel with
   four `strb` under `+strict-align`. The conversion has no `unsafe`.
4. **Doubled rows copied.** Each source line is converted once and the next
   row filled with `copy_from_slice` (`memcpy`).
5. **Unchanged frames skipped.** On a platform that lends its buffer,
   `I_FinishUpdate` returns early when the palette-indexed picture and the
   finished palette equal the last presented ones (a word-by-word compare
   against a 64 KB copy; the target's `memcmp` is a byte loop and cost 3 %
   of the run in a first version).

Why Wood drew twice per tic: Room's `TryRunTics` keeps doomgeneric's wait
loop, which calls `NetUpdate` (building the next tic) and then returns
without running it as soon as the clock passed the entry tic (`I_GetTime() /
ticdup - entertic > 0`, "the menu is still running"). Waiting for the next tic
therefore always ends in one `D_Display` with no new tic, and the tic runs on
the next call. Chocolate Doom later guarded that check with "still no tics";
the port stays faithful to doomgeneric, and the skip makes the extra frame
cost a compare instead of a conversion and a commit.

### Numbers (QEMU `wood-test`, 150 s, no keys, `libhotpc.so`)

The other components changed between the runs (the lava lamp now pauses when
it loses the focus to Wood), so the shares of the whole run are not
comparable; Wood's own counts per tic are. The A/B row differs from "after"
only in the skip being disabled.

| run | Wood instructions | per tic | `I_FinishUpdate` calls | frames presented (per s) | compositor | lava lamp |
|---|---:|---:|---:|---:|---:|---:|
| before | 88.0 G (56.8 %) | 17.0 M | 10 203 | 10 203 (≈ 69) | 38.0 G (24.6 %) | 15.0 G (9.7 %) |
| after, skip disabled (items 1–4) | 18.5 G (27.4 %) | 3.57 M | 10 383 | 10 383 (≈ 70) | 33.6 G (49.7 %) | 8 M |
| after (items 1–5) | 15.5 G (23.2 %) | 2.99 M | 10 404 | 5 980 (≈ 40) | 35.1 G (52.6 %) | 8 M |

All three ran 5 187–5 195 tics (35 per second).

- **Per presented frame** the path went from 7.56 M instructions
  (`I_FinishUpdate` 4.49 M + `draw_frame` 3.08 M) to about 0.80 M: 456 k in
  `convert_frame`, about 290 k in `memcpy` (the doubled rows and the 64 KB
  copy of the picture), 22 k per call in `I_FinishUpdate` itself (mostly the
  compare).
- **Item 5's measured effect:** 43 % of the frames are skipped (4 424 of
  10 404), Wood's instructions drop by 16 % (18.5 → 15.5 G, 3.57 → 2.99 M
  per tic), and commits to the compositor drop from 10 385 to 5 982. The
  compositor's work does **not** drop (33.6 → 35.1 G, within run-to-run
  noise): it composes on its fixed 33 ms timer while there is damage, so
  commits above 30 per second never cost it a composition.
- Of the 5 980 presented frames, about 1 000 came without a new tic but with
  a changed picture: the wipes' melt frames (the first wipe alone is 31) and
  others not identified.
- **Visible change:** none expected; a skipped frame is identical by
  construction, and the compositor shows at most 30 per second. The
  `test-wood` screen dump shows the menu over the demo with the right
  colours.
- Item 4 is not the cheapest form: the row copy costs about 290 k per frame
  through the target's `memcpy`, while a second `stp` to the next row in the
  conversion loop would cost about 64 k.

## Files

- `userspace/dispatcher/src/{lib,main}.rs`, `tests/policy.rs`: the priority policy;
  `libs/device-driver` (`Priority`, `driver_threads!`, `DispatchInit::priorities`),
  `libs/image-build`, `libs/image`, `kernel/kickstart/src/drivers.rs`: the priority's
  path; `userspace/tests/driver/probe`: its QEMU check.
- `userspace/wood/vesper/src/sound.rs`: priority, pre-fill, 8 buffers;
  `userspace/wood/room/src/engine/sound/mixer.rs`: proposal 5;
  `userspace/shell/src/display.rs`: the lamp's pause.
- `userspace/wood/synth/src/voice.rs`, `sf2.rs` and `synth.rs`: proposal 4.
- `userspace/wood/synth/tests/doom_music.rs`: `profile_render` (ignored test),
  `unaligned_bank_renders_the_same`.
- `Justfile`: `profile-synth`.
- Throwaway, not in the repository: `/tmp/profile-plugin/{hotpc.c,percall.c,attribute.py}`;
  logs `target/profile-{midi,wood,wood2}*.log`.
