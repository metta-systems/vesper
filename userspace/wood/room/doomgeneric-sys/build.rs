//! Build script for `doomgeneric-sys`.
//!
//! Compiles the vendored doomgeneric C source files into a static library
//! using the [`cc`] crate. The compiled library provides the core Doom
//! engine along with the doomgeneric platform abstraction layer, but
//! **without** the platform-specific display/input implementation
//! (DG_Init, DG_DrawFrame, etc.), which must be provided by the linking
//! binary (i.e. the `room` crate).
//!
//! ## Build flags
//!
//! The C code is compiled with the following preprocessor definitions,
//! matching the original doomgeneric Linux/X11 build:
//!
//! | Flag | Purpose |
//! |------|---------|
//! | `NORMALUNIX` | Enable POSIX / Unix code paths |
//! | `LINUX` | Linux-specific system calls |
//! | `_DEFAULT_SOURCE` | Expose POSIX extensions from glibc |
//!
//! The `FEATURE_MULTIPLAYER` flag is intentionally **not** defined, which
//! causes all networking code to be compiled out via `#ifdef` guards.
//!
//! Sound (`FEATURE_SOUND`) is also **not** defined, which is consistent
//! with the doomgeneric approach of leaving sound as optional.

use std::path::PathBuf;

fn main() {
    let vendor = PathBuf::from("../vendor/doomgeneric");

    // Emit cargo rerun-if-changed directives so the build is incremental.
    println!("cargo:rerun-if-changed=../vendor/doomgeneric");

    // The complete set of C source files to compile, matching the
    // doomgeneric Makefile (minus the platform-specific files).
    // Platform files (doomgeneric_xlib.c, doomgeneric_sdl.c, etc.) are
    // excluded because the `room` crate provides its own implementation
    // of the DG_* functions via Rust.
    // Build the main library (everything except layout_probe.c).
    let lib_sources: &[&str] = &[
        // Stub / dummy implementations (networking, etc.)
        // dummy  — ported to Rust (room/src/doom/dummy.rs)
        // Automap — ported to Rust (room/src/doom/am_map.rs)
        // "am_map.c",
        // Doom definitions & state
        // doomdef  — removed, no symbols
        // doomstat — ported to Rust (room/src/doom/doomstat.rs)
        // String tables
        // dstrings — ported to Rust (room/src/doom/dstrings.rs)
        // Events — ported to Rust (room/src/doom/d_event.rs)
        // "d_event.c",
        // Items — ported to Rust (room/src/doom/d_items.rs)
        // "d_items.c",
        // IWAD loading — ported to Rust (room/src/doom/d_iwad.rs)
        // "d_iwad.c",
        // Main game loop — ported to Rust (room/src/doom/d_loop.rs)
        // "d_loop.c",
        // Main entry point and game loop — ported to Rust (room/src/doom/d_main.rs)
        // "d_main.c",
        // Game mode detection
        // d_mode   — ported to Rust (room/src/doom/d_mode.rs)
        // Networking stub — ported to Rust (room/src/doom/d_net.rs)
        // "d_net.c",
        // Finale / end screens — ported to Rust (room/src/doom/f_finale.rs)
        // "f_finale.c",
        // Screen wipe effect — ported to Rust (room/src/doom/f_wipe.rs)
        // "f_wipe.c",
        // g_game.c — ported to Rust (room/src/doom/g_game.rs)
        // HUD text library — ported to Rust (room/src/doom/hu_lib.rs)
        // "hu_lib.c",
        // Thing info tables
        // info      — ported to Rust (room/src/doom/info.rs)
        // CD music stub
        // i_cdmus  — ported to Rust (room/src/doom/i_cdmus.rs)
        // ENDOOM screen
        // i_endoom — ported to Rust (room/src/doom/i_endoom.rs)
        // Joystick stub
        // i_joystick — ported to Rust (room/src/doom/i_joystick.rs)
        // Sound stub
        // i_sound   — ported to Rust (room/src/doom/i_sound.rs)
        // System functions (error handling, etc.) — ported to Rust (room/src/doom/i_system.rs)
        // "i_system.c",
        // Timer
        // i_timer   — ported to Rust (room/src/doom/i_timer.rs)
        // Miscellaneous I/O
        // memio — ported to Rust (room/src/doom/memio.rs)
        // Command-line argument parsing
        // m_argv    — ported to Rust (room/src/doom/m_argv.rs)
        // Bounding box — ported to Rust (room/src/doom/m_bbox.rs)
        // Cheat codes
        // m_cheat  — ported to Rust (room/src/doom/m_cheat.rs)
        // m_config  — ported to Rust (room/src/doom/m_config.rs)
        // Control bindings
        // m_controls — ported to Rust (room/src/doom/m_controls.rs)
        // Fixed-point math — ported to Rust (room/src/doom/m_fixed.rs)
        // Menus
        // m_menu  — ported to Rust (room/src/doom/m_menu.rs)
        // M_Menu_SetPlayerMessage moved to room/src/doom/d_player.rs
        // Miscellaneous utilities
        // m_misc    — ported to Rust (room/src/doom/m_misc.rs)
        // Variadic helpers replaced by M_StringJoinA/M_snprintf_clamp in Rust + macros in m_misc.h
        // Random number generator — ported to Rust (room/src/doom/m_random.rs)
        // Ceiling actions — ported to Rust (room/src/doom/p_ceilng.rs)
        // "p_ceilng.c",
        // Door actions — ported to Rust (room/src/doom/p_doors.rs)
        // "p_doors.c",
        // AI / enemy logic — ported to Rust (room/src/doom/p_enemy.rs)
        // "p_enemy.c",
        // Floor actions — ported to Rust (room/src/doom/p_floor.rs)
        // "p_floor.c",
        // Player interactions — ported to Rust (room/src/doom/p_inter.rs)
        // "p_inter.c",
        // Lighting effects — ported to Rust (room/src/doom/p_lights.rs)
        // "p_lights.c",
        // Map collisions — ported to Rust (room/src/doom/p_map.rs)
        // "p_map.c",
        // Map utility functions — ported to Rust (room/src/doom/p_maputl.rs)
        // "p_maputl.c",
        // Map objects (things) — ported to Rust (room/src/doom/p_mobj.rs)
        // "p_mobj.c",
        // Moving platforms — ported to Rust (room/src/doom/p_plats.rs)
        // "p_plats.c",
        // Player sprite logic — ported to Rust (room/src/doom/p_pspr.rs)
        // "p_pspr.c",
        // Save games — ported to Rust (room/src/doom/p_saveg.rs)
        // "p_saveg.c",
        // Map loading — ported to Rust (room/src/doom/p_setup.rs)
        // "p_setup.c",
        // Line-of-sight checks — ported to Rust (room/src/doom/p_sight.rs)
        // "p_sight.c",
        // Special actions — ported to Rust (room/src/doom/p_spec.rs)
        // "p_spec.c",
        // Switch actions — ported to Rust (room/src/doom/p_switch.rs)
        // "p_switch.c",
        // Teleporter — ported to Rust (room/src/doom/p_telept.rs)
        // "p_telept.c",
        // Thinker / object tick — ported to Rust (room/src/doom/p_tick.rs)
        // "p_tick.c",
        // Player movement — ported to Rust (room/src/doom/p_user.rs)
        // "p_user.c",
        // Binary space partitioner traversal — ported to Rust (room/src/doom/r_bsp.rs)
        // "r_bsp.c",
        // Texture / flat data — ported to Rust (room/src/doom/r_data.rs)
        // "r_data.c",
        // Column / span drawing — ported to Rust (room/src/doom/r_draw.rs)
        // "r_draw.c",
        // Renderer main — ported to Rust (room/src/doom/r_main.rs)
        // "r_main.c",
        // Visplane rendering — ported to Rust (room/src/doom/r_plane.rs)
        // "r_plane.c",
        // Segment rendering — ported to Rust (room/src/doom/r_segs.rs)
        // "r_segs.c",
        // Sky rendering — ported to Rust (room/src/doom/r_sky.rs)
        // "r_sky.c",
        // Sprite rendering — ported to Rust (room/src/doom/r_things.rs)
        // "r_things.c",
        // SHA-1 hash (for WAD checksums)
        // sha1 — ported to Rust (room/src/doom/sha1.rs)
        // Sound data tables
        // sounds — ported to Rust (room/src/doom/sounds.rs)
        // Intermission stats
        // statdump  — ported to Rust (room/src/doom/statdump.rs)
        // Status bar library — ported to Rust (room/src/doom/st_lib.rs)
        // "st_lib.c",
        // Status bar — ported to Rust (room/src/doom/st_stuff.rs)
        // "st_stuff.c",
        // Sound subsystem (no-op when FEATURE_SOUND is not defined)
        // s_sound   — ported to Rust (room/src/doom/s_sound.rs)
        // Trigonometry tables
        // tables — ported to Rust (room/src/doom/tables.rs)
        // Video / screen buffer management — ported to Rust (room/src/doom/v_video.rs)
        // "v_video.c",
        // Intermission / victory screens
        // wi_stuff — ported to Rust (room/src/doom/wi_stuff.rs)
        // "wi_stuff.c",
        // WAD checksum
        // w_checksum — ported to Rust (room/src/doom/w_checksum.rs)
        // WAD file abstraction — ported to Rust (room/src/doom/w_file.rs)
        // "w_file.c",
        // "w_file_stdc.c",
        // WAD main loader — ported to Rust (room/src/doom/w_main.rs)
        // "w_main.c",
        // WAD directory — ported to Rust (room/src/doom/w_wad.rs)
        // "w_wad.c",
        // Zone memory allocator — ported to Rust (room/src/doom/z_zone.rs)
        // "z_zone.c",
        // Input handling (calls DG_GetKey) — ported to Rust (room/src/doom/i_input.rs)
        // "i_input.c",
        // Video output (calls DG_DrawFrame, DG_Init) — ported to Rust (room/src/doom/i_video.rs)
        // "i_video.c",
        // doomgeneric glue — ported to Rust (room/src/doom/doomgeneric.rs)
        // "doomgeneric.c",
    ];

    let mut build = cc::Build::new();

    build
        // Use the vendor directory for include resolution.
        .include(&vendor)
        // Match the original doomgeneric Linux build flags.
        .define("NORMALUNIX", None)
        .define("LINUX", None)
        .define("_DEFAULT_SOURCE", None)
        // Do NOT define FEATURE_MULTIPLAYER – networking code is compiled
        // out via #ifdef guards throughout the source.
        // Do NOT define FEATURE_SOUND – sound is out of scope.
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-sign-compare")
        .flag_if_supported("-Wno-implicit-fallthrough")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-maybe-uninitialized");

    // If the user is running with AddressSanitizer on the Rust side, also
    // instrument the C code so that overflows in C are reported with exact
    // line numbers instead of being hidden behind the FFI boundary.
    if std::env::var("ASAN").is_ok() {
        build
            .flag_if_supported("-fsanitize=address")
            .flag_if_supported("-fno-omit-frame-pointer")
            .flag_if_supported("-g");
    }

    for src in lib_sources {
        build.file(vendor.join(src));
    }

    // Exposes C #define constants and struct layout values for Rust layout-guard tests.
    build.file("test_helpers.c");

    build.compile("doomgeneric");

    // Link against libm for math functions used by the engine.
    println!("cargo:rustc-link-lib=m");
}
