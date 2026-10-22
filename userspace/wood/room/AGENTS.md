# Agent instructions

# Memory Safety & Debugging

## Lessons from C → Rust Porting

- **Null-terminated strings for C FFI**: Pass `&[u8] = b"/\0"` instead of `&str = "/"` when C code will call `strlen` on the pointer. `&str` is not null-terminated and causes `global-buffer-overflow`.
- **Avoid `ptr::write_bytes` for precise zeroing**: `ptr::write_bytes` compiles to `memset`, which under ASan may use SIMD writes that overshoot non-aligned sizes and corrupt adjacent allocator metadata. Use a byte-by-byte loop instead.
- **Have allocators zero internally**: Rather than relying on callers to `memset`, zero user data inside `Z_Malloc` before returning.
- **Struct padding fields and zero-initialization**: When porting C structs that contain private `_pad` fields, you cannot use struct literal syntax. Use `std::mem::zeroed()` or `MaybeUninit::zeroed().assume_init()` instead.
- **Moving globals between ported modules**: When a global was previously accessed via `extern "C"` in one Rust module and the C source gets ported, move the `#[no_mangle] pub static mut` definition to the new module and update the consumer to access it directly (e.g., `crate::doom::r_things::spryscale`).
- **Unsigned angle arithmetic wrapping**: C `angle_t` is `u32`, and subtraction/addition can wrap around zero. Rust's default `-` and `+` on `u32` panic in debug mode. Always use `wrapping_sub`, `wrapping_add`, `wrapping_neg` for angle arithmetic.
- **Opaque `state_t` vs concrete `State` struct**: `d_player.rs` declares `state_t` as an empty enum for FFI, but `info.rs` defines the real `State` struct. When accessing state fields from ported code, cast the pointer to `*mut State`.

## AddressSanitizer

ASan gives exact line numbers for memory corruption across the Rust/C boundary. It is especially useful for `unsafe`, FFI, raw pointers, manual buffers, and ownership mistakes.

### Quick Start

```bash
# Via Taskfile
task asan:test -- test_name -- --nocapture

# Manual
ASAN_OPTIONS="detect_leaks=1:halt_on_error=1:abort_on_error=1:symbolize=1" \
RUST_BACKTRACE=1 \
RUSTFLAGS="-Zsanitizer=address" \
cargo +nightly test -Zbuild-std --target x86_64-unknown-linux-gnu
```

Use `-Zbuild-std` so `std` is also instrumented. Compile the C side with ASan too by setting `ASAN=1` (gated in `doomgeneric-sys/build.rs`).

### Useful `ASAN_OPTIONS` Flags

| Flag | Meaning |
|------|---------|
| `detect_leaks=1` | Also report leaks |
| `halt_on_error=1` | Stop at first error |
| `abort_on_error=1` | Generate a hard crash for debuggers/agents |
| `symbolize=1` | Print readable stack traces |

### What ASan Reports Look Like

```text
ERROR: AddressSanitizer: heap-use-after-free
READ of size 8 at 0x...
    #0 my_crate::module::function src/foo.rs:123
    #1 my_crate::ffi_wrapper::call src/ffi.rs:45

freed by thread T0 here:
    #0 free
    #1 native_destroy src/native/foo.c:88

previously allocated by thread T0 here:
    #0 malloc
    #1 native_create src/native/foo.c:42
```

ASan reports three locations: the bad access, the free, and the allocation.

### Limitations

- Needs nightly Rust.
- Slows execution and increases memory use.
- May conflict with proc macros or dynamic libraries.
- Best on Linux/macOS x86_64/aarch64.
- Does not replace Miri or prove memory safety.

## c2rust Intermediate Reference

A fully-automated `c2rust transpile` output lives in `c2rust-intermediate/`. It is **not** linked into the main binary and is excluded from the default workspace build (`default-members`). Its sole purpose is as a behavioural reference when porting or debugging C modules.

### Regenerating

```bash
./tools/c2rust-transpile.sh
```

This script:
1. Scans `vendor/doomgeneric/*.c` and filters out non-transpilable files.
2. Emits `compile_commands.json` with the exact flags from `doomgeneric-sys/build.rs`.
3. Runs `c2rust transpile --emit-build-files --overwrite-existing`.
4. Fixes up `Cargo.toml` and `src/lib.rs` so the crate is usable.

### Checking the reference crate

The transpiled code requires nightly because c2rust emits `extern type` declarations (still unstable):

```bash
cargo +nightly check -p c2rust-intermediate
```

### Excluded files

| File | Reason |
|------|--------|
| `layout_probe.c` | Explicitly excluded from upstream Makefile |
| `gusconf.c` | Requires `FEATURE_SOUND` |
| `m_misc.c` | Contains variadic macros (`M_StringJoin`, `M_vsnprintf`) that crash c2rust |
| `dummy.c` | Empty stub |
| `doomdef.c` | Header-only in practice, no symbols |

### How to use it

- **Type layout validation**: Compare `#[repr(C)]` struct definitions against `room/src/doom/c_ffi.rs`.
- **Behavioural comparison**: When a hand-ported module behaves differently, compare its logic to the transpiled version (which faithfully reproduces C semantics).
- **Symbol inventory**: See exactly which functions, globals, and types a given C module exports before porting it.

### Caveats

- All code is `unsafe` and non-idiomatic — do not copy-paste into the hand-ported codebase.
- Duplicate type definitions exist across modules (e.g. `mobj_t` appears in many files). This is expected because each transpiled file is standalone.
- `#define` values are baked in at transpile time.
- Cross-module calls remain `extern "C"` FFI; there are no Rust `use` imports between modules.

## dhat Heap Profiler

For callsite-level allocation tracking (volume and ownership paths), use `dhat` separately from ASan.

Enable in `room/Cargo.toml`:

```toml
[features]
dhat-heap = ["dep:dhat"]

[dependencies]
dhat = { version = "0.3", optional = true }
```

Run:

```bash
cargo run --features dhat-heap
cargo test --test demo_playthrough --features dhat-heap
```

A `dhat-heap.json` file is produced; view it with the [dhat viewer](https://valgrind.org/docs/manual/dh-manual.html).

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **room** (72941 symbols, 88747 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## When Debugging

1. `gitnexus_query({query: "<error or symptom>"})` — find execution flows related to the issue
2. `gitnexus_context({name: "<suspect function>"})` — see all callers, callees, and process participation
3. `READ gitnexus://repo/room/process/{processName}` — trace the full execution flow step by step
4. For regressions: `gitnexus_detect_changes({scope: "compare", base_ref: "main"})` — see what your branch changed

## When Refactoring

- **Renaming**: MUST use `gitnexus_rename({symbol_name: "old", new_name: "new", dry_run: true})` first. Review the preview — graph edits are safe, text_search edits need manual review. Then run with `dry_run: false`.
- **Extracting/Splitting**: MUST run `gitnexus_context({name: "target"})` to see all incoming/outgoing refs, then `gitnexus_impact({target: "target", direction: "upstream"})` to find all external callers before moving code.
- After any refactor: run `gitnexus_detect_changes({scope: "all"})` to verify only expected files changed.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Tools Quick Reference

| Tool | When to use | Command |
|------|-------------|---------|
| `query` | Find code by concept | `gitnexus_query({query: "auth validation"})` |
| `context` | 360-degree view of one symbol | `gitnexus_context({name: "validateUser"})` |
| `impact` | Blast radius before editing | `gitnexus_impact({target: "X", direction: "upstream"})` |
| `detect_changes` | Pre-commit scope check | `gitnexus_detect_changes({scope: "staged"})` |
| `rename` | Safe multi-file rename | `gitnexus_rename({symbol_name: "old", new_name: "new", dry_run: true})` |
| `cypher` | Custom graph queries | `gitnexus_cypher({query: "MATCH ..."})` |

## Impact Risk Levels

| Depth | Meaning | Action |
|-------|---------|--------|
| d=1 | WILL BREAK — direct callers/importers | MUST update these |
| d=2 | LIKELY AFFECTED — indirect deps | Should test |
| d=3 | MAY NEED TESTING — transitive | Test if critical path |

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/room/context` | Codebase overview, check index freshness |
| `gitnexus://repo/room/clusters` | All functional areas |
| `gitnexus://repo/room/processes` | All execution flows |
| `gitnexus://repo/room/process/{name}` | Step-by-step execution trace |

## Self-Check Before Finishing

Before completing any code modification task, verify:
1. `gitnexus_impact` was run for all modified symbols
2. No HIGH/CRITICAL risk warnings were ignored
3. `gitnexus_detect_changes()` confirms changes match expected scope
4. All d=1 (WILL BREAK) dependents were updated

## Keeping the Index Fresh

After committing code changes, the GitNexus index becomes stale. Re-run analyze to update it:

```bash
npx gitnexus analyze
```

If the index previously included embeddings, preserve them by adding `--embeddings`:

```bash
npx gitnexus analyze --embeddings
```

To check whether embeddings exist, inspect `.gitnexus/meta.json` — the `stats.embeddings` field shows the count (0 means no embeddings). **Running analyze without `--embeddings` will delete any previously generated embeddings.**

> Claude Code users: A PostToolUse hook handles this automatically after `git commit` and `git merge`.

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
