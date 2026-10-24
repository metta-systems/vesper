# Room part 4: explicit world borrows instead of `Global`s + raw pointers

Based on the tree after the player-index step (`PlayerId`, 2026-10-24).
All twelve decisions in section 6 were answered with the recommended
option. Sub-steps are tracked in section 5.

## 0. Where we are (measured)

- `unsafe fn` (non-comment lines containing `unsafe fn`, `src` + `tests`):
  521 (declaration-only regex: 503).
  By module: play 217, interface 45, render_3d 44, intermission 34,
  status 32, game.rs 23, automap 21, hud 20, doom 16, zone 13, finale 12,
  wad 11, misc 9, memio 9, sound 7.
  Inside play: enemy 65, saveg 35, pspr 26, map 22, maputl 19, mobj 16,
  inter 8, spec 7, user 5, doors 4, floor 4, ceilng/plats/info 2 each.
- Arena handle dereferences `Handle::ptr()`: about 300 sites (play 280:
  enemy 83, saveg 39, map 24, mobj 23, doors 16, tick 14, inter 13,
  maputl 10, user 10, lights 8, ...). 15 of them take a long-lived
  `&mut *id.ptr()` for a whole body (mobj.rs 11).
- `PlayerId::ptr()`: 17 sites (pspr 7, user 4, status 3, inter 2, game 1).
- Simulation state in `Global`s: `LEVEL` (142 `borrow`/`borrow_mut`
  sites), `SPAWN_SPOTS`, `SETUP`, `TICK` (leveltime + thinker list),
  nine thinker arenas (`MOBJ_ARENA` ... `FIRE_FLICKER_ARENA`), `MOBJ`
  (item respawn queue), `MAP` (`tm*` scratch, spechit), `MAPUTL` + 
  `INTERCEPTS` (trace, openings), `SIGHT`, `ENEMY`, `PSPR`, `USER`,
  `SPEC`, `SWITCHES`, `CEILINGS`, `PLATS`, `PLAYERS`, `VALIDCOUNT`,
  plus the sim-facing part of `GAME` and `RANDOM`.
- Other raw pointers the sim still carries (orthogonal to world borrows,
  but they keep functions `unsafe`): `*mut State` (11 decl sites,
  `mobj_t.state`, `PspdefT.state`), `*mut MobjInfo` (`mobj_t.info`, ~60
  uses), `*mut intercept_t` (10), `*mut mapthing_t` (8), ~80 raw
  `*mut u8/i16/i32/c_void` in play + render_3d (lumps, columns,
  openings).

## 1. What `World` contains

`World` = everything the play simulation reads or writes during a tic and
that a save game captures. One struct, `pub` fields, so callers can split
borrows by field (`let World { level, mobjs, .. } = w;`).

```text
World {
    level:     LevelState          // LEVEL (+ SPAWN_SPOTS, playerstarts)
    mobjs:     Arena<mobj_t>       // MOBJ_ARENA
    thinkers:  Thinkers {          // TICK + the 8 other arenas
        list: Vec<ThinkerId>, ceilings, doors, floors, plats,
        light_flashes, strobes, glows, fire_flickers }
    leveltime: i32
    players:   [PlayerT; MAXPLAYERS]   // PLAYERS
    session:   Session             // copy of gameskill, deathmatch, netgame,
                                   // respawnmonsters, fastparm, nomonsters,
                                   // gamemap, gameepisode, consoleplayer,
                                   // playeringame[], totalkills/items/secret,
                                   // bodyque/bodyqueslot
    random:    PRandom             // prndindex only (decision Q6)
    active:    ActiveSpecials {    // CEILINGS, PLATS, SWITCHES (buttonlist),
        ceilings, plats, buttons, item_respawn_queue (MOBJ) }
    scratch:   PlayScratch {       // MAP, MAPUTL, INTERCEPTS, SIGHT, ENEMY,
        map, maputl, intercepts, sight, enemy, pspr, user, validcount }
}
```

Stays global (not simulation, or process-wide):
config/`CONFIG`, `CONTROLS`, `MENU`/`MENU_PAGES`, `VIDEO`, `SCALE`,
`INPUT`, `JOYSTICK`, `TIMER`, `SYSTEM`, `ARGV`, `SOUND` and
`SOUND_INTERFACE` (channels, device), `SOUND_INFO`, `WAD`, `ZONE`,
`PLATFORM`, `DOOM_MAIN`, `MAIN_LOOP`, `NET`, `EVENT`, `STAT`,
`GAME_INPUT`, the controller part of `GAME` (gameaction, gamestate,
demo flags/buffers, wminfo), `HUD`, `STATUS`, `AUTOMAP`, `INTERMISSION`,
`STATDUMP`, `FINALE`, `WIPE`, `CD_MUSIC`, `M_Random`'s `rndindex`.
`STATES`/`MOBJINFO` are DeHackEd-patched once at startup, then
read-only (decision Q8). Renderer scratch (`VIEW`, `BSP`, `CLIP_SEGS`,
`DRAW`, `PLANES`, `OPENINGS`, `WALL`, `SPRITES`, `VISSPRITES`, `SKY`,
`RENDER_DATA`) becomes a separate `Renderer` context, not part of
`World`.

## 2. Entry points: where `&mut World` / `&World` comes from

Recommended (decision Q1): `pub static WORLD: Global<World>`, borrowed
once at each top-level entry and passed down as a parameter. The debug
borrow counter then flags any re-entrant top-level borrow.

| Entry | Borrow | Notes |
|---|---|---|
| `G_Ticker` | per phase: `&mut` for reborn/`G_DoReborn`, `P_Ticker(&mut w)`, `&World` for `ST_Ticker`/`AM_Ticker`, `&mut` short for `HU_Ticker` (clears `message`) | game actions (`G_DoLoadLevel`, `G_DoLoadGame`, `G_DoSaveGame`, `G_DoCompleted`) borrow `&mut` inside their own body |
| `P_Ticker(w)` | given | players, thinkers, specials, respawns |
| `P_SetupLevel(w, ...)` | given by `G_DoLoadLevel` | clears arenas, loads level, spawns things |
| save / load (`P_Archive*`, `P_UnArchive*`) | `&World` / `&mut World` | |
| `G_Responder` -> `ST_Responder` cheats, `HU_Responder`, `AM_Responder` | short `&mut` per event | events are processed inside `NetUpdate`, which the renderer calls between phases (below) |
| `D_Display` -> `R_RenderPlayerView` | `&World` per render phase, none across `NetUpdate` | |
| `doomgeneric_Tick` -> `S_UpdateSounds` | `&World` | listener + origin positions |

Do not hold a `World` guard across `TryRunTics` (it calls `RunTic` ->
`G_Ticker` -> borrows) or across `NetUpdate` (processes events ->
responders -> cheats borrow `&mut`). The renderer already calls
`NetUpdate` between its phases, so it borrows per phase: setup frame,
BSP, planes, masked.

## 3. The hard cases

### 3.1 Re-entrant play code
`P_TryMove` -> `P_CheckPosition` -> `P_BlockThingsIterator(PIT_CheckThing)`
-> `P_TouchSpecialThing` / `P_DamageMobj` -> `P_KillMobj` ->
`P_SetMobjState` -> `A_*` -> `P_SpawnMobj` / `P_RemoveMobj` /
`P_SpawnMissile` -> `P_CheckMissileSpawn` -> `P_TryMove` ... Also
`P_LineAttack` -> `P_PathTraverse(PTR_ShootTraverse)` -> `P_DamageMobj`,
`P_RadiusAttack` -> `PIT_RadiusAttack`, `P_ChangeSector` ->
`PIT_ChangeSector` -> `P_DamageMobj`/spawn blood, `P_CrossSpecialLine` ->
`EV_*` -> new thinkers, weapon actions -> `P_SetPsprite` -> actions.
Practically all of play (enemy, map, maputl, mobj, inter, pspr, user,
spec, doors, floor, ceilng, plats, telept, switch, lights, sight, tick) is
one strongly connected component of the call graph.

Rule: every function in the component takes `w: &mut World` and never
holds a reference into `w` across a call that takes `w`. Field access is
per expression (`w.mobjs[id].x`, `w.mobjs[id].flags |= ...`); values
needed after a call are copied into locals before it (copy-out) and
written back after it (write-back), re-reading anything the call may have
changed, which is what C does through its pointers anyway. Arena slots are
never reused within a level, so a `MobjId` stays a valid index after
spawns and removals (stale handles keep reading the freed data, as
today). The borrow checker enforces the rule, which is exactly the
soundness argument the `.ptr()` comments make by hand today.

### 3.2 Two objects mutably (actor + target)
Most sites read one and write the other: copy the read side into locals
(`let (tx, ty) = { let t = &w.mobjs[target]; (t.x, t.y) };`). For the
few that write both at once, `Arena::get2_mut(a, b) -> (&mut T, &mut T)`
with an explicit `a == b` branch (C permits the same object, e.g. a
monster damaged by its own blast; `P_DamageMobj` checks
`source != target` only for retargeting). Unit-test the alias branch.

### 3.3 State actions and thinker dispatch
- `StateAction::Mobj(fn(&mut World, MobjId))`,
  `StateAction::Weapon(fn(&mut World, PlayerId, PsprSlot))` where
  `PsprSlot` is the psprite index (0 weapon, 1 flash) instead of
  `*mut PspdefT`.
- `run_thinker(w, ThinkerId)` dispatches `T_MoveCeiling(w, id)` etc.
  `run_thinker_list` already holds no borrow of the list while a thinker
  runs (index walk, `thinker_at`), so it ports directly to
  `w.thinkers.list`.

### 3.4 Blockmap / path-traverse callbacks
- `P_BlockThingsIterator(w, x, y, f: impl FnMut(&mut World, MobjId) -> bool)`:
  walk `cur = w.level.blocklinks[i]`, call `f(w, id)`, then read
  `w.mobjs[id].bnext` after the call (C reads `mobj->bnext` after the
  callback; a removed thing keeps its data).
- `P_BlockLinesIterator(w, x, y, f: impl FnMut(&mut World, LineId) -> bool)`:
  index loop over the blockmap lump offset, reading one entry per
  iteration (no slice borrow held across `f`).
- `P_PathTraverse` / `P_TraverseIntercepts`: intercepts live in
  `w.scratch.intercepts`; pass `intercept_t` by value to the callback
  (it is small and `Copy`) instead of `*mut intercept_t`.
- The `PIT_*`/`PTR_*` callbacks keep reading their parameters from
  `w.scratch.map` (`tmthing`, `tmx`, ...), as C does. Turning them into
  closure captures is a later, optional cleanup (decision Q3b).
- Hazard: `InterceptsOverrun` writes object addresses
  (`Handle::address()`, line addresses) into emulated memory. Keep an
  address-like value available after arenas move (e.g. a stable
  `Box<[Slot]>` chunk address, as today).

### 3.5 Sound from play code
`S_StartSound(&World, origin, sfx)`: play passes `&*w` (a shared
reborrow) for the call; sound state stays in its own global. Reads the
console player's `mo` and the origin/listener positions from `w`
(decision Q4).

### 3.6 HUD / status / automap reading player state
- Tickers and drawers take `&World` (status/automap) or a short `&mut`
  (HUD clears `player.message`; AM_Responder sets messages).
- `st_lib` widgets keep `*mut i32` into the player slot and `STATUS`
  (`w_ready.num`, health, armor, ammo, maxammo, weaponowned). Rework them
  to receive the value at update/draw time (`STlib_updateNum(widget,
  value)`), which removes the stable-address requirement on `PLAYERS`
  (decision Q12).

### 3.7 Renderer reading mobjs/level
Play never runs while a frame is drawn, so the renderer can take `&World`
plus its own `&mut Renderer`. Two renderer writes into the level block a
plain `&World`: `line.flags |= ML_MAPPED` (segs.rs, saved in save games
and read by the automap) and `sector.validcount` (things.rs, sprite
de-dup). Options in decision Q5.

### 3.8 Game controller code calling play
`G_CheckSpot`, `G_DoReborn`, `G_DeathMatchSpawnPlayer`,
`G_PlayerFinishLevel`, `G_DoCompleted`, cheats: take `&mut World` from
their caller (G_Ticker phase) or borrow once at their own top.

## 4. Aliasing strategy summary

1. One `&mut World` parameter through the play component; no reference
   into it survives a call that takes it (borrowck-enforced).
2. Short expression-level field access on arenas via `Index`/`IndexMut`
   by handle (`w.mobjs[id]`), with stale-read semantics unchanged.
3. Copy-out / write-back for multi-field work around calls.
4. `get2_mut` for the rare true two-object writes.
5. Disjoint field borrows (`&mut w.level` + `&w.mobjs`) inside leaf
   functions that do not call back (renderer, sight, line openings).
6. Entry points borrow `WORLD` once; debug builds panic on any second
   borrow, catching missed re-entrance.

## 5. Ordered sub-steps

Each step compiles, keeps save bytes, and runs: `just test` (debug: the
`Global` borrow counter catches re-entrance), `cargo test --release`,
the demo baseline `tests/demo_playthrough.rs`, the save round-trip test
(`saveg.rs` thinker/player round trip), clippy kinds, and the vesper
check.

| # | Step | Size | Risk | Extra checks | `unsafe fn` effect |
|---|---|---|---|---|---|
| 4.0 | Add a golden save test: run the demo to tic N, `G_DoSaveGame` into memory, compare bytes with a fixture captured now; plus a load-and-continue snapshot test | S | low | new test itself | 0 |
| 4.1 | `mobj_t.state`/`PspdefT.state` -> `Option<StateNum>`, `mobj_t.info` -> `MobjType` index; `STATES`/`MOBJINFO` frozen read-only after DeHackEd | M (~150 sites) | low-med (thinker identity by function pointer, `P_SetPsprite` loop) | demo, golden save | -15..-25 (pure table derefs) |
| 4.2 | Introduce `World` (empty) + `WORLD` global + entry-point borrows; thread `w: &mut World` through the whole play component, action/thinker/iterator signatures (`fn(&mut World, ...)`), keeping bodies on `.ptr()` | L (~220 fn signatures, ~1500 call sites), mechanical | low (no behaviour change) | demo, debug-run entry-borrow test | 0 |
| 4.3 | Players into `w.players`; remove `PlayerId::ptr()` from play; UI via `&World` | S-M | low | demo, save round trip | -10..-15 (pspr/user/inter/spec) |
| 4.4 | Play scratch into `w.scratch` (MAP, MAPUTL, INTERCEPTS, SIGHT, ENEMY, PSPR, USER, VALIDCOUNT), `w.leveltime`, active-special lists, session copy, P_Random index; iterators become closures over `&mut World`; intercepts by value | M-L | med (iterator semantics, overrun emulation) | demo; unit tests: removal during blockmap walk continues via `bnext`; intercept order | -20..-30 (maputl, map callbacks) |
| 4.5 | Level into `w.level` (142 `LEVEL.borrow*` sites); renderer gets `&mut Level`/`&World` per Q5 | M | low-med | demo; automap mapped-lines test | -5..-10 |
| 4.6 | Mobj access through `w.mobjs[id]` instead of `.ptr()`, file by file (lights, telept, switch, plats, ceilng, floor, doors, spec, sight, inter, user, pspr, mobj, map, maputl, enemy, saveg); `get2_mut` where needed | XL (~300 sites) | med-high (aliasing bugs show as behaviour changes, not UB) | demo after each file; stale-access report must not grow | -120..-150 (the bulk of play) |
| 4.7 | Thinker arenas owned by `w.thinkers`; delete `Handle::ptr()` and `ThinkerId::thinker()` raw pointer; `Pooled` arenas no longer `Global` | M | med | demo, save round trip, arena tests | -15..-25 |
| 4.8 | Renderer: `R_RenderPlayerView(&World, &mut Renderer, PlayerId)`, per-phase borrows around `NetUpdate`; renderer scratch out of globals | L | med | demo frame hash (add a framebuffer checksum at fixed tics to the demo test, if allowed: Q11) | -10..-20 (column/span drawers keep raw pixel pointers) |
| 4.9 | UI: status widgets take values, HUD/automap/intermission read `&World` | M | low | demo, status unit tests | -15..-25 |
| 4.10 | Sweep: drop `unsafe` from functions with no unsafe op left (compile-driven: remove keyword, keep only where errors), rewrite `# Safety` sections, consider re-enabling `unsafe_op_in_unsafe_fn` per converted module | S-M | low | clippy | -30..-60 (functions only callers-unsafe) |

Expected end state: play and UI largely safe; remaining `unsafe fn`
concentrated in interface (platform, video, sound device), zone/memio,
WAD lump parsing, renderer column/span drawers, and the memory-overrun
emulations. Rough total after part 4: 520 -> ~200-250.

Biggest risk: step 4.6 (behavioural drift from a missed write-back).
Mitigation: convert one file per commit, demo after each, keep the
stale-access debug report as a canary, and the golden save from 4.0.

## 6. Decisions

Each question below was answered with its recommendation.

Q1. Where does `World` live?
- (a) `static WORLD: Global<World>`, borrowed once at entry points.
- (b) Owned by the platform loop and passed through `doomgeneric_Tick`,
  `TryRunTics`, the `RunTic` loop-interface callback and `D_Display`.
- (c) `thread_local!`.
Recommendation: (a) now; (b) is possible later but needs the
`loop_interface` callback table reworked.

Q2. One `World` or separate contexts (`&mut Level`, `&mut Mobjs`,
`&mut Players`, `&mut PlayScratch`) as parameters?
- (a) One `&mut World` with public fields; split borrows inside leaf
  functions.
- (b) Separate parameters everywhere.
Recommendation: (a); the play component needs nearly everything, (b)
multiplies signatures without real disjointness.

Q3. Conversion order.
- (a) Thread an empty `World` token through signatures first (4.2), then
  move state in (4.3-4.7).
- (b) Move one `Global` at a time and convert its users in the same
  step.
Recommendation: (a); (b) cannot work inside the play call-graph cycle
without a big-bang step.
Q3b. Keep `PIT_*` parameters in `w.scratch.map` (C style) or capture
them in closures? Recommendation: keep in scratch for part 4 (some are
read after the iterator returns: `tmfloorz`, `ceilingline`,
`linetarget`), closures later.

Q4. Sound calls from play.
- (a) Immediate, `S_StartSound(&World, ...)` with a shared reborrow.
- (b) Queue in `World`, flush after the tic (listener position read at
  end of tic, not mid-tic).
Recommendation: (a), identical behaviour.

Q5. Renderer writes into the level (`ML_MAPPED` line flag,
`sector.validcount`).
- (a) Renderer takes `&mut World` (or `&mut Level`).
- (b) Move both into renderer-owned side tables (mapped bitset, sector
  validcount array); automap reads the bitset; save game merges the
  mapped bit back into the saved line flags so bytes stay the same.
- (c) `Cell`s in `line_t`/`sector_t` for these two fields.
Recommendation: (a) first (smallest), (b) later if a read-only world for
the renderer is wanted.

Q6. Random number state.
- (a) `prndindex` into `World` (it is simulation state), `rndindex`
  (`M_Random`, UI) stays global.
- (b) Keep `RANDOM` global.
Recommendation: (a); the demo test reads `RANDOM.get()` and would need
its accessor changed (see Q11).

Q7. Session settings read by play (`gameskill`, `deathmatch`, `netgame`,
`respawnmonsters`, `fastparm`, `nomonsters`, `gamemap`, ...).
- (a) Copy into `World.session` at `G_InitNew`/`G_DoLoadLevel`/load.
- (b) Keep reading `GAME` from play.
Recommendation: (a); play then needs nothing from `GAME`.

Q8. `STATES` / `MOBJINFO`.
- (a) Freeze to `&'static` after DeHackEd at startup.
- (b) Fields of `World`.
Recommendation: (a); they are not per-level and not saved.

Q9. Stale-handle semantics once arenas are owned by `World`: keep
"freed slot keeps its data, no reuse within a level" and the debug
stale-access report? Recommendation: keep both (vanilla behaviour
depends on it; the report is the canary for step 4.6).

Q10. Re-enable `unsafe_op_in_unsafe_fn` for converted modules in 4.10?
Recommendation: yes, module by module.

Q11. `tests/demo_playthrough.rs` is frozen. May its accessors change
(as in this step: `player(i)` -> `PlayerId::read`) and may it gain
checks (framebuffer checksum at fixed tics, golden save bytes)?
Recommendation: allow accessor changes and added checks, never changed
expected values.

Q12. Status bar widgets.
- (a) Rework `st_lib` to take values at update time.
- (b) Keep stable pointers into `WORLD` (valid while it is a `Global`).
Recommendation: (a); (b) ties `World` to a fixed address forever.
