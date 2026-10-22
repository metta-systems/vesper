# Replace cross-module `extern "C"` with `use` imports - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace every cross-module `extern "C"` declaration inside `room/src/doom/` with `use crate::doom::module::Symbol` imports, eliminating the FFI indirection for intra-crate calls.

**Architecture:** All modules in `room/src/doom/` are submodules of the same `room` crate. Cross-module calls via `extern "C"` blocks are a legacy of the C-to-Rust port. Each consuming file gets `use` imports that reference the defining module directly. The `pub extern "C"` function definitions and all libc/doomgeneric callback declarations remain unchanged.

**Tech Stack:** Rust, `cargo check -p room`, `cargo test -p room`

---

## Classification rules (apply in every task)

For each declaration inside an `extern "C" { }` block, classify it as one of:

| Class | Characteristic | Action |
|-------|---------------|--------|
| **Cross-module Rust** | `pub extern "C" fn NAME` or `pub static mut NAME` exists in another `room/src/doom/*.rs` file | Replace with `use crate::doom::MODULE::NAME;` |
| **Real C FFI** | libc symbol (`strlen`, `malloc`, `free`, `fopen`, `snprintf`, …) or doomgeneric callback (`DG_Init`, `DG_DrawFrame`, `DG_SetWindowTitle`, `DG_GetKey`, `DG_SleepMs`, `DG_GetTicksMs`) | Keep in `extern "C"` block |

After replacing all cross-module entries from a block, remove the block entirely if it is empty. If the block still has real C FFI entries, keep it.

Call sites (`unsafe { SYMBOL(...) }`) do not change. `pub unsafe extern "C" fn` remains `unsafe` when imported via `use`, so existing unsafe blocks continue to compile.

**Lookup command** (run from repo root when in doubt):

```bash
grep -rn "pub extern \"C\" fn\|pub unsafe extern \"C\" fn\|#\[no_mangle\]\s*\npub static\|^pub static mut" \
  room/src/doom/ --include="*.rs" | grep -v "//\|test"
```

---

## Task 1: Worked example — `p_tick.rs` (closes #58)

**Files:**
- Modify: `room/src/doom/p_tick.rs:50-60`

- [ ] **Step 1: Confirm the extern "C" block content**

```bash
grep -n "extern \"C\"" -A 15 room/src/doom/p_tick.rs
```

Expected output shows one block (lines 50-60):
```
extern "C" {
    fn Z_Free(ptr: *mut c_void);
    fn P_PlayerThink(player: *mut PlayerT);
    fn P_UpdateSpecials();
    fn P_RespawnSpecials();
    static mut paused: c_int;
    static mut netgame: c_int;
    static mut menuactive: c_int;
    static mut demoplayback: c_int;
    static mut playeringame: [c_int; MAXPLAYERS];
}
```

- [ ] **Step 2: Locate each symbol's defining module**

```bash
grep -rn "pub.*fn Z_Free\|pub.*fn P_PlayerThink\|pub.*fn P_UpdateSpecials\|pub.*fn P_RespawnSpecials" \
  room/src/doom/ --include="*.rs" | grep -v "//\|test"
grep -rn "pub static mut paused\|pub static mut netgame\|pub static mut menuactive\|pub static mut demoplayback\|pub static mut playeringame" \
  room/src/doom/ --include="*.rs" | grep -v "//"
```

Expected:
- `Z_Free` -> `z_zone.rs`
- `P_PlayerThink` -> `p_user.rs`
- `P_UpdateSpecials` -> `p_spec.rs`
- `P_RespawnSpecials` -> `p_mobj.rs`
- `paused`, `netgame`, `playeringame`, `demoplayback` -> `g_game.rs`
- `menuactive` -> `m_menu.rs`

All 9 entries are cross-module Rust symbols. The whole block is replaced.

- [ ] **Step 3: Replace the extern "C" block with use imports**

In `room/src/doom/p_tick.rs`, replace lines 50-60:

```rust
// REMOVE this entire block:
extern "C" {
    fn Z_Free(ptr: *mut c_void);
    fn P_PlayerThink(player: *mut PlayerT);
    fn P_UpdateSpecials();
    fn P_RespawnSpecials();
    static mut paused: c_int;
    static mut netgame: c_int;
    static mut menuactive: c_int;
    static mut demoplayback: c_int;
    static mut playeringame: [c_int; MAXPLAYERS];
}
```

Add to the imports at the top of the file (after the existing `use crate::doom::d_player::{...}` line):

```rust
use crate::doom::g_game::{demoplayback, netgame, paused, playeringame};
use crate::doom::m_menu::menuactive;
use crate::doom::p_mobj::P_RespawnSpecials;
use crate::doom::p_spec::P_UpdateSpecials;
use crate::doom::p_user::P_PlayerThink;
use crate::doom::z_zone::Z_Free;
```

Remove any `use std::ffi::c_void` or `use std::os::raw::c_int` lines that are now unused (the compiler will warn about them).

- [ ] **Step 4: Run cargo check**

```bash
cargo check -p room
```

Expected: no errors. Warnings about unused imports are fine — fix them by removing unused `use` lines.

- [ ] **Step 5: Commit**

```bash
git add room/src/doom/p_tick.rs
git commit -m "refactor(doom): replace extern \"C\" with use imports in p_tick.rs

Closes #58"
```

---

## Task 2: Physics files — `p_floor.rs`, `p_doors.rs`, `p_ceilng.rs`, `p_lights.rs`, `p_plats.rs` (closes #52, #53, #54, #55, #56)

Process each file sequentially. For each:

**Step pattern (repeat for each file):**

- [ ] **Step 1: Show extern "C" block**

```bash
grep -n "extern \"C\"" -A 30 room/src/doom/FILE.rs
```

- [ ] **Step 2: Look up each declared symbol**

```bash
# For each function NAME in the block:
grep -rn "pub.*fn NAME\|pub static mut NAME" room/src/doom/ --include="*.rs" | grep -v "//"
```

- [ ] **Step 3: Add use imports, remove block entries**

For any symbol found in another `room/src/doom/*.rs` file, add:
```rust
use crate::doom::MODULE::SYMBOL;
```

Remove those entries from the `extern "C"` block. Remove the block if empty. Keep entries for libc functions (if any).

- [ ] **Step 4: cargo check**

```bash
cargo check -p room
```

Fix any unused import warnings by removing the corresponding `use` line.

- [ ] **Step 5: Commit each file**

```bash
git add room/src/doom/FILE.rs
git commit -m "refactor(doom): replace extern \"C\" with use imports in FILE.rs

Closes #ISSUE"
```

File-to-issue mapping:
- `p_floor.rs` -> #52
- `p_doors.rs` -> #53
- `p_ceilng.rs` -> #54
- `p_lights.rs` -> #55
- `p_plats.rs` -> #56

**Known cross-module symbols in this cluster:**

`Z_Malloc` (z_zone.rs), `S_StartSound` (s_sound.rs), `P_FindSectorFromLineTag` (p_spec.rs), `P_ChangeSector` (p_spec.rs), `P_FindNextHighestFloor` (p_spec.rs), `P_FindHighestFloorSurrounding` (p_spec.rs), `P_FindLowestFloorSurrounding` (p_spec.rs), `P_FindLowestCeilingSurrounding` (p_spec.rs), `getSide` (p_maputl.rs), `getSector` (p_maputl.rs), `twoSided` (p_maputl.rs), `T_MovePlane` (p_floor.rs or p_spec.rs — verify), `T_PlatRaise` (p_plats.rs)

---

## Task 3: Physics files — `p_sight.rs`, `p_user.rs`, `p_spec.rs`, `p_setup.rs` (closes #57, #51, #32, #33)

Apply the step pattern from Task 2 for each file. File-to-issue mapping:
- `p_sight.rs` -> #57
- `p_user.rs` -> #51
- `p_spec.rs` -> #32
- `p_setup.rs` -> #33

**Step pattern:** same as Task 2.

- [ ] Process `p_sight.rs`: grep block, lookup symbols, add uses, remove block, cargo check, commit #57
- [ ] Process `p_user.rs`: grep block, lookup symbols, add uses, remove block, cargo check, commit #51
- [ ] Process `p_spec.rs`: grep block, lookup symbols, add uses, remove block, cargo check, commit #32
- [ ] Process `p_setup.rs`: grep block, lookup symbols, add uses, remove block, cargo check, commit #33

---

## Task 4: Physics files — `p_map.rs`, `p_mobj.rs`, `p_pspr.rs`, `p_enemy.rs`, `p_switch.rs`, `p_telept.rs`, `p_inter.rs` (closes #38, #40, #41, #42, #43, #47, #48)

Apply the step pattern from Task 2 for each file.

- [ ] Process `p_map.rs` -> commit #38
- [ ] Process `p_mobj.rs` -> commit #40
- [ ] Process `p_pspr.rs` -> commit #41
- [ ] Process `p_enemy.rs` -> commit #42
- [ ] Process `p_switch.rs` -> commit #43
- [ ] Process `p_telept.rs` -> commit #47
- [ ] Process `p_inter.rs` -> commit #48

**Known cross-module symbols:**
- `p_enemy.rs` block contains: `G_ExitLevel` (g_game.rs), `A_ReFire` (p_pspr.rs), `EV_DoDoor` (p_doors.rs), `EV_DoFloor` (p_floor.rs)

---

## Task 5: Renderer files — `r_main.rs`, `r_bsp.rs`, `r_plane.rs`, `r_segs.rs`, `r_data.rs`, `r_things.rs` (closes #31, #59, #60, #61, #45, #46)

Apply the step pattern from Task 2 for each file.

- [ ] Process `r_main.rs` -> commit #31
- [ ] Process `r_bsp.rs` -> commit #59
- [ ] Process `r_plane.rs` -> commit #60
- [ ] Process `r_segs.rs` -> commit #61
- [ ] Process `r_data.rs` -> commit #45
- [ ] Process `r_things.rs` -> commit #46

---

## Task 6: Game, Sound, Finale, Main — `g_game.rs`, `s_sound.rs`, `f_finale.rs`, `d_main.rs` (closes #50, #35, #37, #34)

Apply the step pattern from Task 2 for each file.

- [ ] Process `g_game.rs` -> commit #50
- [ ] Process `s_sound.rs` -> commit #35
- [ ] Process `f_finale.rs` -> commit #37
- [ ] Process `d_main.rs` -> commit #34

---

## Task 7: HUD/Status/Video — `st_stuff.rs`, `wi_stuff.rs`, `v_video.rs` (closes #39, #49, #44)

Apply the step pattern from Task 2 for each file.

- [ ] Process `st_stuff.rs` -> commit #39
- [ ] Process `wi_stuff.rs` -> commit #49
- [ ] Process `v_video.rs` -> commit #44

---

## Task 8: Menu/Config — `m_menu.rs`, `m_config.rs`, `m_misc.rs`, `m_argv.rs` (closes #36, #62, #63, #64)

Apply the step pattern from Task 2 for each file.

- [ ] Process `m_menu.rs` -> commit #36
- [ ] Process `m_config.rs` -> commit #62
- [ ] Process `m_misc.rs` -> commit #63
- [ ] Process `m_argv.rs` -> commit #64

---

## Task 9: System/Input/Scale — `i_video.rs`, `i_system.rs`, `i_sound.rs`, `i_scale.rs`, `i_timer.rs` (closes #65, #66, #67, #68, #69)

Apply the step pattern from Task 2 for each file.

- [ ] Process `i_video.rs` -> commit #65
- [ ] Process `i_system.rs` -> commit #66
- [ ] Process `i_sound.rs` -> commit #67
- [ ] Process `i_scale.rs` -> commit #68
- [ ] Process `i_timer.rs` -> commit #69

---

## Task 10: Audit files — `z_zone.rs`, `w_wad.rs`, `w_checksum.rs`, `w_file.rs`, `w_main.rs`, `memio.rs`, `statdump.rs`, `st_lib.rs` (closes #70-#77)

These files need inspection before classification. For each:

- [ ] **Step 1: Show the full extern "C" block**

```bash
grep -n "extern \"C\"" -A 40 room/src/doom/FILE.rs
```

- [ ] **Step 2: Classify each entry**

For each declared symbol:
```bash
# Check if it exists as a Rust function/static in another module:
grep -rn "pub.*fn SYMBOL\b\|pub static mut SYMBOL\b" room/src/doom/ --include="*.rs" | grep -v "//"
# Check if it's a real C symbol (libc or doomgeneric callback):
# If not found above AND name looks like libc (strlen, malloc, fopen, etc.) -> keep
```

- [ ] **Step 3: Apply changes**

Replace cross-module entries with `use` imports. Keep real C FFI entries. Remove empty blocks.

- [ ] **Step 4: cargo check**

```bash
cargo check -p room
```

- [ ] **Step 5: Commit each file**

```bash
git add room/src/doom/FILE.rs
git commit -m "refactor(doom): audit and replace extern \"C\" in FILE.rs

Closes #ISSUE"
```

File-to-issue mapping:
- `z_zone.rs` -> #70
- `w_wad.rs` -> #71
- `w_checksum.rs` -> #72
- `w_file.rs` -> #73
- `w_main.rs` -> #74
- `memio.rs` -> #75
- `statdump.rs` -> #76
- `st_lib.rs` -> #77

**Known case:** `memio.rs` declares `fn Z_Malloc` and `fn Z_Free` — both are in `z_zone.rs` and will be replaced.

---

## Task 11: Final verification and PR

- [ ] **Step 1: Run full test suite**

```bash
cargo test -p room
```

Expected: all tests pass.

- [ ] **Step 2: Run integration test**

```bash
cargo test --test demo_playthrough
```

Expected: passes (or skipped if no WAD file present in CI).

- [ ] **Step 3: Verify no extern "C" cross-module declarations remain**

Run this to confirm no cross-module `extern "C"` blocks are left. Any remaining `extern "C"` blocks should contain only libc or doomgeneric symbols:

```bash
# List all remaining extern "C" blocks with content
grep -rn "extern \"C\" {" room/src/doom/ --include="*.rs" -A 10
```

Review each remaining block and confirm all entries are libc or doomgeneric callbacks.

- [ ] **Step 4: Create PR**

```bash
gh pr create \
  --title "refactor(doom): replace cross-module extern \"C\" with use imports" \
  --body "$(cat <<'EOF'
## Summary

Replaces all cross-module `extern "C"` declarations within `room/src/doom/` with
idiomatic Rust `use crate::doom::module::Symbol` imports.

- All `pub extern "C"` function definitions are preserved (required for doomgeneric-sys FFI)
- All libc and doomgeneric callback declarations in `extern "C"` blocks are preserved
- Call sites are unchanged

Closes #30
Closes #31, #32, #33, #34, #35, #36, #37, #38, #39, #40
Closes #41, #42, #43, #44, #45, #46, #47, #48, #49, #50
Closes #51, #52, #53, #54, #55, #56, #57, #58, #59, #60
Closes #61, #62, #63, #64, #65, #66, #67, #68, #69
Closes #70, #71, #72, #73, #74, #75, #76, #77

## Test plan

- [x] `cargo check -p room` after each file
- [x] `cargo test -p room` passes
- [x] `cargo test --test demo_playthrough` passes
- [x] No cross-module `extern "C"` declarations remain in `room/src/doom/`
EOF
)"
```
