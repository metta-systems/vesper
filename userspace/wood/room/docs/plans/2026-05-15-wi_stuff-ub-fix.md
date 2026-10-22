# wi_stuff UB Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate undefined behavior in `wi_stuff.rs` caused by writing through pointers derived from immutable `static` animation tables.

**Architecture:** Promote three immutable `static` animation info tables to `static mut`, change `ANIMS` to hold `*mut anim_t`, remove all const-to-mut casts at write sites, and use a proper read-only pointer at the draw site. Remove the now-unnecessary `unsafe impl Sync for anim_t`.

**Tech Stack:** Rust (no new dependencies), `cargo test --lib` for verification.

**Spec:** `docs/specs/2026-05-15-wi_stuff-ub-fix-design.md`

---

### Task 1: Add animation-table structural tests (TDD baseline)

These tests verify structural invariants before and after the fix. They must pass both before and after your changes — if they fail before the fix, something else is wrong.

**Files:**
- Modify: `room/src/doom/c_tests/wi_stuff_c.rs`

- [ ] **Step 1: Add tests for animation table entry counts**

At the end of `room/src/doom/c_tests/wi_stuff_c.rs`, add:

```rust
// ---------------------------------------------------------------------------
// Animation table constants
// ---------------------------------------------------------------------------

/// Episode 0 (Knee-Deep in the Dead) has 10 animated background elements.
#[test]
fn epsd0_animinfo_count() {
    assert_eq!(c_ffi::WI_EPSD0_NANIM, 10);
}

/// Episode 1 (The Shores of Hell) has 9 animated background elements.
#[test]
fn epsd1_animinfo_count() {
    assert_eq!(c_ffi::WI_EPSD1_NANIM, 9);
}

/// Episode 2 (Inferno) has 6 animated background elements.
#[test]
fn epsd2_animinfo_count() {
    assert_eq!(c_ffi::WI_EPSD2_NANIM, 6);
}
```

- [ ] **Step 2: Export the constants from `c_ffi.rs`**

In `room/src/doom/c_ffi.rs`, find the `// wi_stuff.c` section (around line 599) and add after the existing wi_stuff constants:

```rust
/// Number of animated elements in episode 0 background (EPSD0ANIMINFO length).
pub const WI_EPSD0_NANIM: usize = 10;
/// Number of animated elements in episode 1 background (EPSD1ANIMINFO length).
pub const WI_EPSD1_NANIM: usize = 9;
/// Number of animated elements in episode 2 background (EPSD2ANIMINFO length).
pub const WI_EPSD2_NANIM: usize = 6;
```

- [ ] **Step 3: Run the new tests — they must pass**

```bash
cargo test --lib doom::c_tests::wi_stuff_c
```

Expected output:
```
test doom::c_tests::wi_stuff_c::epsd0_animinfo_count ... ok
test doom::c_tests::wi_stuff_c::epsd1_animinfo_count ... ok
test doom::c_tests::wi_stuff_c::epsd2_animinfo_count ... ok
```

- [ ] **Step 4: Commit**

```bash
git add room/src/doom/c_ffi.rs room/src/doom/c_tests/wi_stuff_c.rs
git commit -m "test(wi_stuff): add animation table structural tests"
```

---

### Task 2: Promote animation tables to `static mut`

**Files:**
- Modify: `room/src/doom/wi_stuff.rs`

Context: `EPSD0ANIMINFO` is at line 188, `EPSD1ANIMINFO` at line 321, `EPSD2ANIMINFO` at line 441. The `unsafe impl Sync for anim_t {}` is at line 141.

- [ ] **Step 1: Remove `unsafe impl Sync for anim_t {}`**

At line 141, remove this line entirely:

```rust
unsafe impl Sync for anim_t {}
```

`static mut` does not require `Sync` — only `static` (shared) items do. All three tables are becoming `static mut`, so this impl is no longer needed.

- [ ] **Step 2: Mark `EPSD0ANIMINFO` as `static mut`**

Change line 188 from:

```rust
static EPSD0ANIMINFO: [anim_t; 10] = [
```

to:

```rust
// TODO: Consider splitting into immutable config fields (type_, period, nanims,
// loc, data1, data2) and mutable runtime state (ctr, nexttic, lastdrawn, state,
// p) using two separate arrays, or wrapping in UnsafeCell for explicit interior
// mutability without static mut.
static mut EPSD0ANIMINFO: [anim_t; 10] = [
```

- [ ] **Step 3: Mark `EPSD1ANIMINFO` as `static mut`**

Change line 321 from:

```rust
static EPSD1ANIMINFO: [anim_t; 9] = [
```

to:

```rust
// TODO: Same as EPSD0ANIMINFO — candidate for config/state split or UnsafeCell.
static mut EPSD1ANIMINFO: [anim_t; 9] = [
```

- [ ] **Step 4: Mark `EPSD2ANIMINFO` as `static mut`**

Change line 441 from:

```rust
static EPSD2ANIMINFO: [anim_t; 6] = [
```

to:

```rust
// TODO: Same as EPSD0ANIMINFO — candidate for config/state split or UnsafeCell.
static mut EPSD2ANIMINFO: [anim_t; 6] = [
```

- [ ] **Step 5: Update `ANIMS` to use `*mut anim_t`**

Change lines 529-534 from:

```rust
static mut ANIMS: [*const anim_t; NUMEPISODES] = [
    EPSD0ANIMINFO.as_ptr(),
    EPSD1ANIMINFO.as_ptr(),
    EPSD2ANIMINFO.as_ptr(),
    ptr::null(),
];
```

to:

```rust
// addr_of_mut! is required here: .as_mut_ptr() is not stable-const-safe
// for static mut arrays in a static initializer context.
static mut ANIMS: [*mut anim_t; NUMEPISODES] = [
    addr_of_mut!(EPSD0ANIMINFO) as *mut anim_t,
    addr_of_mut!(EPSD1ANIMINFO) as *mut anim_t,
    addr_of_mut!(EPSD2ANIMINFO) as *mut anim_t,
    ptr::null_mut(),
];
```

- [ ] **Step 6: Verify the crate compiles**

```bash
cargo build -p room
```

Expected: no errors. Warnings about `static_mut_refs` are already suppressed by the existing `#![allow(...)]` at the top of the file.

- [ ] **Step 7: Commit**

```bash
git add room/src/doom/wi_stuff.rs
git commit -m "fix(wi_stuff): promote animation tables to static mut, closes #20"
```

---

### Task 3: Remove const-to-mut casts at write sites

Now that `ANIMS` holds `*mut anim_t`, the casts are unnecessary and can be removed.

**Files:**
- Modify: `room/src/doom/wi_stuff.rs`

- [ ] **Step 1: Fix `WI_initAnimatedBack`**

Around line 761, change:

```rust
    let base = ANIMS[epsd] as *mut anim_t;
```

to:

```rust
    let base = ANIMS[epsd];
```

- [ ] **Step 2: Fix `WI_updateAnimatedBack`**

Around line 786, change:

```rust
    let base = ANIMS[epsd] as *mut anim_t;
```

to:

```rust
    let base = ANIMS[epsd];
```

- [ ] **Step 3: Fix `WI_loadUnloadData` — first cast**

Around line 1599, change:

```rust
                let a = (ANIMS[(*wbs).epsd as usize] as *mut anim_t).add(j as usize);
```

to:

```rust
                let a = ANIMS[(*wbs).epsd as usize].add(j as usize);
```

- [ ] **Step 4: Fix `WI_loadUnloadData` — second cast (cross-episode patch alias)**

Around line 1605, change:

```rust
                        (*a).p[i as usize] = (*((ANIMS[1] as *mut anim_t).add(4))).p[i as usize];
```

to:

```rust
                        (*a).p[i as usize] = (*ANIMS[1].add(4)).p[i as usize];
```

- [ ] **Step 5: Fix `WI_drawAnimatedBack` — read-only path**

Around line 833, change:

```rust
    let base = ANIMS[epsd] as *mut anim_t;
```

to:

```rust
    let base = ANIMS[epsd] as *const anim_t;
```

Then on the next iteration line (835-836), change:

```rust
    for i in 0..count {
        let a = base.add(i);
```

This works as-is because `*const anim_t` also has `.add()`. No further change needed on those lines.

- [ ] **Step 6: Build and verify**

```bash
cargo build -p room
```

Expected: no errors.

- [ ] **Step 7: Run all tests**

```bash
cargo test --lib
```

Expected: all 752+ tests pass, including the 3 new wi_stuff animation table tests from Task 1.

- [ ] **Step 8: Commit**

```bash
git add room/src/doom/wi_stuff.rs
git commit -m "refactor(wi_stuff): remove const-to-mut casts now that tables are static mut"
```

---

### Task 4: Final verification

- [ ] **Step 1: Run full test suite**

```bash
cargo test
```

Expected: all tests pass, zero failures.

- [ ] **Step 2: Confirm no `as *mut anim_t` casts remain on ANIMS read**

```bash
grep -n "ANIMS.*as \*mut" room/src/doom/wi_stuff.rs
```

Expected: no output. If any lines appear, they are remaining UB-prone casts that need to be fixed.

- [ ] **Step 3: Confirm `unsafe impl Sync for anim_t` is gone**

```bash
grep -n "impl Sync for anim_t" room/src/doom/wi_stuff.rs
```

Expected: no output.
