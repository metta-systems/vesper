# Design: Replace cross-module `extern "C"` with `use` imports

**Epic:** https://github.com/sunsided/room/issues/30
**Branch:** `feature/imports`
**Date:** 2026-05-15

## Problem

`room/src/doom/` has ~78 files with `extern "C"` blocks. Many of these blocks declare functions and statics that are already defined in other Rust modules in the same crate. This is a legacy from the C-to-Rust port where cross-file calls were done through `extern "C"` because each file started as a standalone C translation unit. Now that all modules live in one Rust crate, these declarations can be replaced with idiomatic `use crate::doom::module::Symbol` imports.

## Scope

- **39 "Replace" sub-issues** (#31-#69): files where cross-module `extern "C"` declarations are confirmed
- **8 "Audit" sub-issues** (#70-#77): files that need inspection before classifying entries as cross-module or real C FFI
- All changes land in one PR on `feature/imports`, closing the epic (#30) and all sub-issues

## What NOT to change

- `pub extern "C"` function definitions - these must stay for `doomgeneric-sys` FFI
- Actual C FFI declarations (libc: `strlen`, `malloc`, `free`, `fopen`, etc.)
- doomgeneric callback declarations (`DG_DrawFrame`, `DG_Init`, `DG_SetWindowTitle`, etc.)
- Call sites - `unsafe { Z_Malloc(...) }` blocks are unchanged; the function remains `unsafe` when imported via `use`

## Architecture

All modules in `room/src/doom/` are submodules of the `room` crate. Cross-module calls do not require `extern "C"` - they can use `use crate::doom::module::Symbol` directly. The `pub extern "C"` ABI on function definitions is preserved for the FFI boundary with `doomgeneric-sys`.

## Per-file transformation algorithm

**Step 1 - Build lookup table (once):**
Scan every `*.rs` file in `room/src/doom/` for:
- `pub extern "C" fn NAME` and `pub unsafe extern "C" fn NAME`
- `#[no_mangle] pub static mut NAME`

Result: map of `NAME -> module_name`.

**Step 2 - Classify each `extern "C"` declaration in the consumer file:**
- Found in lookup table: cross-module Rust symbol -> replace with `use`
- Not found (libc, doomgeneric): real C FFI -> keep in `extern "C"` block

**Step 3 - Edit the file:**
- Add `use crate::doom::MODULE::NAME;` for each cross-module symbol
- Remove those entries from the `extern "C"` block
- If block becomes empty, remove the entire `extern "C" { }` block
- If block has both C FFI and Rust entries, keep C FFI entries and remove only the Rust ones
- Do not change call sites

**Step 4 - Verify:** `cargo check -p room`

**Step 5 - Commit:** one commit per file

## Commit strategy

- One commit per file that has actual replacements
- Files where all `extern "C"` entries are real C FFI: no commit needed, just close the sub-issue
- Commit message subject: `refactor(doom): replace extern "C" with use imports in X.rs`
- Commit body: reference sub-issue number

## Testing

- `cargo check -p room` after each file (fast type check)
- `cargo test -p room` after all files are done
- `cargo test --test demo_playthrough` (integration test) as final gate

## Sub-issues

### "Replace" sub-issues (confirmed cross-module declarations)

| Issue | File |
|-------|------|
| #31 | r_main.rs |
| #32 | p_spec.rs |
| #33 | p_setup.rs |
| #34 | d_main.rs |
| #35 | s_sound.rs |
| #36 | m_menu.rs |
| #37 | f_finale.rs |
| #38 | p_map.rs |
| #39 | st_stuff.rs |
| #40 | p_mobj.rs |
| #41 | p_pspr.rs |
| #42 | p_enemy.rs |
| #43 | p_switch.rs |
| #44 | v_video.rs |
| #45 | r_data.rs |
| #46 | r_things.rs |
| #47 | p_telept.rs |
| #48 | p_inter.rs |
| #49 | wi_stuff.rs |
| #50 | g_game.rs |
| #51 | p_user.rs |
| #52 | p_floor.rs |
| #53 | p_doors.rs |
| #54 | p_ceilng.rs |
| #55 | p_lights.rs |
| #56 | p_plats.rs |
| #57 | p_sight.rs |
| #58 | p_tick.rs |
| #59 | r_bsp.rs |
| #60 | r_plane.rs |
| #61 | r_segs.rs |
| #62 | m_config.rs |
| #63 | m_misc.rs |
| #64 | m_argv.rs |
| #65 | i_video.rs |
| #66 | i_system.rs |
| #67 | i_sound.rs |
| #68 | i_scale.rs |
| #69 | i_timer.rs |

### "Audit" sub-issues (inspect and classify first)

| Issue | File |
|-------|------|
| #70 | z_zone.rs |
| #71 | w_wad.rs |
| #72 | w_checksum.rs |
| #73 | w_file.rs |
| #74 | w_main.rs |
| #75 | memio.rs |
| #76 | statdump.rs |
| #77 | st_lib.rs |
