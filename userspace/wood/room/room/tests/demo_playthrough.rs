//! Synthetic Demo Playthrough Test
//!
//! Drives the Doom engine headlessly for ~30 s of virtual time,
//! captures snapshots of player / RNG / game state at checkpoint tics,
//! and compares them against a frozen baseline.
//!
//! Usage:
//!   cargo test --test demo_playthrough        — compare vs BASELINE
//!   BLESS=1 cargo test --test demo_playthrough — print new BASELINE to stdout

#![allow(non_snake_case, non_upper_case_globals)]

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: room::dhat::Alloc = room::dhat::Alloc;

use std::cell::Cell;
use std::ffi::{c_char, c_int, c_uint, CString};
use std::mem::offset_of;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// headless:: TLS re-exports
// ---------------------------------------------------------------------------

thread_local! {
    static VIRTUAL_MS: Cell<u32> = const { Cell::new(1000) };
    static FRAMES: Cell<u64> = const { Cell::new(0) };
}

const TICK_MS: u32 = 1000 / 35;

fn virtual_ms() -> u32 {
    VIRTUAL_MS.with(|v| {
        let old = v.get();
        v.set(old + TICK_MS);
        old
    })
}

fn note_frame() {
    FRAMES.with(|f| f.set(f.get() + 1));
}

fn frame_count() -> u64 {
    FRAMES.with(|f| f.get())
}

// ---------------------------------------------------------------------------
// DG_* stubs (C-callable, replaces room/src/platform/mod.rs)
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn DG_Init() {}

#[no_mangle]
pub extern "C" fn DG_DrawFrame() {
    note_frame();
}

#[no_mangle]
pub extern "C" fn DG_SleepMs(_ms: u32) {}

#[no_mangle]
pub extern "C" fn DG_GetTicksMs() -> u32 {
    virtual_ms()
}

#[no_mangle]
pub extern "C" fn DG_GetKey(_pressed: *mut i32, _doom_key: *mut u8) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn DG_SetWindowTitle(_title: *const c_char) {}

// ---------------------------------------------------------------------------
// C global declarations
// ---------------------------------------------------------------------------

extern "C" {
    static gametic: c_int;
    static gamestate: c_int;
    static mut prndindex: c_int;
    static mut singletics: c_uint;
    static mut longtics: c_uint;
}

// ---------------------------------------------------------------------------
// MobjPrefix – repr-C mirror of the first fields of mobj_s
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct MobjPrefix {
    thinker_prev: *mut (),
    thinker_next: *mut (),
    thinker_fn: *mut (),
    x: i32,
    y: i32,
    z: i32,
    snext: *mut (),
    sprev: *mut (),
    angle: u32,
}

#[test]
fn mobj_prefix_offsets() {
    assert_eq!(offset_of!(MobjPrefix, x), 24, "thinker size mismatch");
    assert_eq!(offset_of!(MobjPrefix, y), 28);
    assert_eq!(offset_of!(MobjPrefix, z), 32);
    assert_eq!(offset_of!(MobjPrefix, angle), 56, "angle offset mismatch");
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Snapshot {
    tic: usize,
    virtual_ms: u32,
    frames: u64,
    gametic: c_int,
    gamestate: c_int,
    rndindex: c_int,
    prndindex: c_int,
    health: c_int,
    armorpoints: c_int,
    killcount: c_int,
    itemcount: c_int,
    secretcount: c_int,
    readyweapon: c_int,
    ammo: [c_int; 4],
    mo_x: i32,
    mo_y: i32,
    mo_z: i32,
    mo_angle: u32,
}

// ---------------------------------------------------------------------------
// BASELINE (paste BLESS output here, then re-run without BLESS)
// ---------------------------------------------------------------------------

const BASELINE: &[Snapshot] = &[
    Snapshot {
        tic: 35,
        virtual_ms: 2064,
        frames: 36,
        gametic: 37,
        gamestate: 3,
        rndindex: 0,
        prndindex: 0,
        health: 0,
        armorpoints: 0,
        killcount: 0,
        itemcount: 0,
        secretcount: 0,
        readyweapon: 0,
        ammo: [0, 0, 0, 0],
        mo_x: 0,
        mo_y: 0,
        mo_z: 0,
        mo_angle: 0,
    },
    Snapshot {
        tic: 70,
        virtual_ms: 3072,
        frames: 71,
        gametic: 72,
        gamestate: 3,
        rndindex: 0,
        prndindex: 0,
        health: 0,
        armorpoints: 0,
        killcount: 0,
        itemcount: 0,
        secretcount: 0,
        readyweapon: 0,
        ammo: [0, 0, 0, 0],
        mo_x: 0,
        mo_y: 0,
        mo_z: 0,
        mo_angle: 0,
    },
    Snapshot {
        tic: 140,
        virtual_ms: 5060,
        frames: 141,
        gametic: 142,
        gamestate: 3,
        rndindex: 0,
        prndindex: 0,
        health: 0,
        armorpoints: 0,
        killcount: 0,
        itemcount: 0,
        secretcount: 0,
        readyweapon: 0,
        ammo: [0, 0, 0, 0],
        mo_x: 0,
        mo_y: 0,
        mo_z: 0,
        mo_angle: 0,
    },
    Snapshot {
        tic: 280,
        virtual_ms: 10240,
        frames: 321,
        gametic: 282,
        gamestate: 0,
        rndindex: 175,
        prndindex: 56,
        health: 98,
        armorpoints: 99,
        killcount: 0,
        itemcount: 0,
        secretcount: 0,
        readyweapon: 1,
        ammo: [48, 0, 0, 0],
        mo_x: -19551832,
        mo_y: -9170049,
        mo_z: 0,
        mo_angle: 352321536,
    },
    Snapshot {
        tic: 560,
        virtual_ms: 18108,
        frames: 601,
        gametic: 562,
        gamestate: 0,
        rndindex: 199,
        prndindex: 230,
        health: 92,
        armorpoints: 99,
        killcount: 4,
        itemcount: 3,
        secretcount: 0,
        readyweapon: 1,
        ammo: [44, 0, 0, 0],
        mo_x: 20134750,
        mo_y: 8949751,
        mo_z: 0,
        mo_angle: 4278190080,
    },
    Snapshot {
        tic: 1050,
        virtual_ms: 31856,
        frames: 1091,
        gametic: 1052,
        gamestate: 0,
        rndindex: 177,
        prndindex: 53,
        health: 83,
        armorpoints: 94,
        killcount: 9,
        itemcount: 4,
        secretcount: 0,
        readyweapon: 2,
        ammo: [39, 10, 0, 0],
        mo_x: 33547877,
        mo_y: 35079301,
        mo_z: 3670016,
        mo_angle: 234881024,
    },
    Snapshot {
        tic: 1500,
        virtual_ms: 44484,
        frames: 1541,
        gametic: 1502,
        gamestate: 0,
        rndindex: 115,
        prndindex: 52,
        health: 90,
        armorpoints: 85,
        killcount: 15,
        itemcount: 4,
        secretcount: 0,
        readyweapon: 2,
        ammo: [49, 9, 0, 0],
        mo_x: 57290564,
        mo_y: 60282469,
        mo_z: 3670016,
        mo_angle: 4261412864,
    },
    Snapshot {
        tic: 1750,
        virtual_ms: 51512,
        frames: 1791,
        gametic: 1752,
        gamestate: 0,
        rndindex: 109,
        prndindex: 220,
        health: 100,
        armorpoints: 200,
        killcount: 15,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 4,
        ammo: [49, 9, 0, 7],
        mo_x: 59985055,
        mo_y: -2948983,
        mo_z: -1572864,
        mo_angle: 2113929216,
    },
    Snapshot {
        tic: 2000,
        virtual_ms: 58540,
        frames: 2041,
        gametic: 2002,
        gamestate: 0,
        rndindex: 103,
        prndindex: 159,
        health: 86,
        armorpoints: 188,
        killcount: 17,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 4,
        ammo: [54, 13, 0, 4],
        mo_x: -1908863,
        mo_y: 40654123,
        mo_z: -1572864,
        mo_angle: 1107296256,
    },
    Snapshot {
        tic: 2250,
        virtual_ms: 65568,
        frames: 2291,
        gametic: 2252,
        gamestate: 0,
        rndindex: 97,
        prndindex: 148,
        health: 13,
        armorpoints: 108,
        killcount: 24,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 4,
        ammo: [64, 13, 0, 1],
        mo_x: -38661769,
        mo_y: 54175013,
        mo_z: 0,
        mo_angle: 2130706432,
    },
    Snapshot {
        tic: 2500,
        virtual_ms: 72596,
        frames: 2541,
        gametic: 2502,
        gamestate: 0,
        rndindex: 91,
        prndindex: 158,
        health: 2,
        armorpoints: 98,
        killcount: 25,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 2,
        ammo: [69, 10, 0, 0],
        mo_x: -103102668,
        mo_y: 26505201,
        mo_z: -12058624,
        mo_angle: 419430400,
    },
    Snapshot {
        tic: 2750,
        virtual_ms: 79624,
        frames: 2791,
        gametic: 2752,
        gamestate: 0,
        rndindex: 85,
        prndindex: 230,
        health: 2,
        armorpoints: 98,
        killcount: 32,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 2,
        ammo: [69, 4, 0, 0],
        mo_x: -97297986,
        mo_y: 29030142,
        mo_z: -12058624,
        mo_angle: 4261412864,
    },
    Snapshot {
        tic: 2800,
        virtual_ms: 81052,
        frames: 2841,
        gametic: 2802,
        gamestate: 0,
        rndindex: 135,
        prndindex: 8,
        health: 2,
        armorpoints: 98,
        killcount: 33,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 2,
        ammo: [69, 19, 0, 0],
        mo_x: -83325358,
        mo_y: 21882974,
        mo_z: -11534336,
        mo_angle: 3556769792,
    },
    Snapshot {
        tic: 2850,
        virtual_ms: 82480,
        frames: 2891,
        gametic: 2852,
        gamestate: 0,
        rndindex: 185,
        prndindex: 186,
        health: 12,
        armorpoints: 98,
        killcount: 33,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 2,
        ammo: [69, 19, 0, 0],
        mo_x: -89996795,
        mo_y: 29427930,
        mo_z: -11534336,
        mo_angle: 2197815296,
    },
    Snapshot {
        tic: 2900,
        virtual_ms: 83908,
        frames: 2941,
        gametic: 2902,
        gamestate: 0,
        rndindex: 235,
        prndindex: 78,
        health: 12,
        armorpoints: 98,
        killcount: 33,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 2,
        ammo: [69, 19, 0, 0],
        mo_x: -109117723,
        mo_y: 36712134,
        mo_z: -12058624,
        mo_angle: 973078528,
    },
    Snapshot {
        tic: 2950,
        virtual_ms: 85336,
        frames: 2991,
        gametic: 2952,
        gamestate: 0,
        rndindex: 29,
        prndindex: 33,
        health: 6,
        armorpoints: 92,
        killcount: 36,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 2,
        ammo: [69, 17, 0, 0],
        mo_x: -108652320,
        mo_y: 57160819,
        mo_z: -12058624,
        mo_angle: 989855744,
    },
    Snapshot {
        tic: 3000,
        virtual_ms: 86764,
        frames: 3041,
        gametic: 3002,
        gamestate: 0,
        rndindex: 79,
        prndindex: 151,
        health: 6,
        armorpoints: 92,
        killcount: 36,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 2,
        ammo: [69, 24, 0, 0],
        mo_x: -107301560,
        mo_y: 72469437,
        mo_z: -12058624,
        mo_angle: 989855744,
    },
    Snapshot {
        tic: 3250,
        virtual_ms: 93792,
        frames: 3291,
        gametic: 3252,
        gamestate: 0,
        rndindex: 73,
        prndindex: 86,
        health: 3,
        armorpoints: 89,
        killcount: 40,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 2,
        ammo: [69, 22, 0, 0],
        mo_x: -91025665,
        mo_y: 81216003,
        mo_z: -11534336,
        mo_angle: 150994944,
    },
    Snapshot {
        tic: 3500,
        virtual_ms: 100820,
        frames: 3541,
        gametic: 3502,
        gamestate: 0,
        rndindex: 67,
        prndindex: 39,
        health: 23,
        armorpoints: 89,
        killcount: 42,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 2,
        ammo: [74, 19, 0, 0],
        mo_x: -107308714,
        mo_y: 61755923,
        mo_z: -12058624,
        mo_angle: 452984832,
    },
    Snapshot {
        tic: 3750,
        virtual_ms: 107848,
        frames: 3791,
        gametic: 3752,
        gamestate: 0,
        rndindex: 61,
        prndindex: 7,
        health: 45,
        armorpoints: 87,
        killcount: 45,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 2,
        ammo: [74, 23, 0, 0],
        mo_x: -74930575,
        mo_y: 58278444,
        mo_z: -12582912,
        mo_angle: 1862270976,
    },
    Snapshot {
        tic: 4000,
        virtual_ms: 114876,
        frames: 4041,
        gametic: 4002,
        gamestate: 0,
        rndindex: 55,
        prndindex: 92,
        health: 34,
        armorpoints: 77,
        killcount: 47,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 2,
        ammo: [79, 21, 0, 0],
        mo_x: -67904886,
        mo_y: 72177779,
        mo_z: -8912896,
        mo_angle: 3254779904,
    },
    Snapshot {
        tic: 4250,
        virtual_ms: 121904,
        frames: 4291,
        gametic: 4252,
        gamestate: 0,
        rndindex: 49,
        prndindex: 187,
        health: 25,
        armorpoints: 68,
        killcount: 48,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 2,
        ammo: [89, 20, 0, 0],
        mo_x: -29900145,
        mo_y: 53456676,
        mo_z: 0,
        mo_angle: 0,
    },
    Snapshot {
        tic: 4500,
        virtual_ms: 128932,
        frames: 4541,
        gametic: 4502,
        gamestate: 0,
        rndindex: 43,
        prndindex: 76,
        health: 14,
        armorpoints: 59,
        killcount: 51,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 2,
        ammo: [94, 14, 0, 0],
        mo_x: 29717865,
        mo_y: 72569780,
        mo_z: 3670016,
        mo_angle: 318767104,
    },
    Snapshot {
        tic: 4750,
        virtual_ms: 135960,
        frames: 4791,
        gametic: 4752,
        gamestate: 0,
        rndindex: 37,
        prndindex: 148,
        health: 11,
        armorpoints: 48,
        killcount: 56,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 2,
        ammo: [99, 8, 0, 0],
        mo_x: 32566652,
        mo_y: 75279227,
        mo_z: 3670016,
        mo_angle: 4278190080,
    },
    Snapshot {
        tic: 5000,
        virtual_ms: 142988,
        frames: 5041,
        gametic: 5002,
        gamestate: 0,
        rndindex: 31,
        prndindex: 80,
        health: 11,
        armorpoints: 48,
        killcount: 58,
        itemcount: 7,
        secretcount: 1,
        readyweapon: 2,
        ammo: [104, 2, 0, 0],
        mo_x: 54608337,
        mo_y: 71725023,
        mo_z: 5242880,
        mo_angle: 201326592,
    },
];

// ---------------------------------------------------------------------------
// Checkpoints
// ---------------------------------------------------------------------------
//
// Dense checkpoints during gameplay to catch subtle state drift.
// The region 2000–3500 is critical — this is where the ported g_game.rs
// causes the player to die (health reaches 0 around tic 2500–3000),
// while the C version keeps them alive at 2–6 HP before they recover.
//
// Key health trajectory (C version, this commit):
//   tic 2250: health=13  (taking heavy damage)
//   tic 2500: health=2   (near death — ported version has health=0 here)
//   tic 2750: health=2   (still hanging on)
//   tic 3000: health=6   (picked up health, recovering)
//   tic 3500: health=23  (fully recovered)
//
// Early checkpoints (title screen → demo start): 35, 70, 140, 280
// Gameplay checkpoints every ~280 tics: 560, 1050
// Dense gameplay checkpoints every 250 tics: 1500..=5000

const CHECKPOINTS: &[usize] = &[
    35, 70, 140, 280, 560, 1050, 1500, 1750, 2000, 2250, 2500, 2750, 2800, 2850, 2900, 2950, 3000,
    3250, 3500, 3750, 4000, 4250, 4500, 4750, 5000,
];
const TOTAL_TICS: usize = 5000;

fn is_checkpoint(tic: usize) -> bool {
    CHECKPOINTS.contains(&tic)
}

// ---------------------------------------------------------------------------
// Snapshot capture
// ---------------------------------------------------------------------------

fn capture_snapshot(tic: usize) -> Snapshot {
    use room::doom::d_player::{consoleplayer, players, MAXPLAYERS};
    use room::doom::m_random::rndindex;

    let cp = unsafe { consoleplayer };
    let pidx = if cp >= 0 && (cp as usize) < MAXPLAYERS {
        cp as usize
    } else {
        0
    };
    let p = unsafe { &players[pidx] };
    let mo_ptr = p.mo as *const MobjPrefix;
    let (mo_x, mo_y, mo_z, mo_angle) = if mo_ptr.is_null() {
        (0, 0, 0, 0)
    } else {
        let mo = unsafe { &*mo_ptr };
        (mo.x, mo.y, mo.z, mo.angle)
    };

    Snapshot {
        tic,
        virtual_ms: virtual_ms(),
        frames: frame_count(),
        gametic: unsafe { gametic },
        gamestate: unsafe { gamestate },
        rndindex: unsafe { rndindex },
        prndindex: unsafe { prndindex },
        health: p.health,
        armorpoints: p.armorpoints,
        killcount: p.killcount,
        itemcount: p.itemcount,
        secretcount: p.secretcount,
        readyweapon: p.readyweapon,
        ammo: p.ammo,
        mo_x,
        mo_y,
        mo_z,
        mo_angle,
    }
}

// ---------------------------------------------------------------------------
// WAD discovery
// ---------------------------------------------------------------------------

fn find_wad() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let wad = manifest
        .join("../doom1.wad")
        .canonicalize()
        .unwrap_or_else(|_| manifest.parent().unwrap_or(&manifest).join("doom1.wad"));
    wad
}

// ---------------------------------------------------------------------------
// The one-and-only test
// ---------------------------------------------------------------------------

#[test]
fn demo_playthrough() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = room::dhat::Profiler::new_heap();

    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .try_init();
    let wad_path = find_wad();
    if !wad_path.exists() {
        panic!(
            "doom1.wad not found at {}; the test requires the shareware IWAD",
            wad_path.display()
        );
    }

    let wad_str = wad_path.to_str().expect("WAD path is not valid UTF-8");

    let argv: Vec<CString> = vec![
        CString::new("room").unwrap(),
        CString::new("-iwad").unwrap(),
        CString::new(wad_str).unwrap(),
        CString::new("-nomusic").unwrap(),
        CString::new("-nosound").unwrap(),
        CString::new("-nomouse").unwrap(),
        CString::new("-nojoy").unwrap(),
        CString::new("-nograbmouse").unwrap(),
    ];

    let mut c_argv: Vec<*mut c_char> = argv.iter().map(|s| s.as_ptr() as *mut c_char).collect();
    c_argv.push(std::ptr::null_mut());

    let argc = (c_argv.len() - 1) as c_int;

    // Enable singletics mode: one game tic per TryRunTics() call,
    // bypassing the real-time waiting loop.
    unsafe {
        singletics = 1;
    }

    // Initialize the Doom engine.
    unsafe {
        doomgeneric_sys::doomgeneric_Create(argc, c_argv.as_mut_ptr());
    }

    // Sanity-check invariants that future agents often re-validate when
    // debugging random-tick divergence between C and Rust.
    assert_eq!(
        room::doom::c_ffi::DOOM_191_VERSION,
        unsafe { doomgeneric_sys::room_test_get_doom_191_version() },
        "Rust DOOM_191_VERSION must match the C #define"
    );
    assert_eq!(
        unsafe { longtics },
        0,
        "longtics should be false when neither -longtics nor a v1.91 demo is loaded"
    );

    let mut snapshots: Vec<Snapshot> = Vec::new();

    // Validate a single snapshot against the baseline, failing fast.
    fn validate_snapshot(i: usize, got: &Snapshot, expected: &Snapshot) {
        let n = i + 1; // 1-indexed for human-readable error messages
        assert_eq!(got.tic, expected.tic, "checkpoint {}: tic mismatch", n);
        assert_eq!(
            got.virtual_ms, expected.virtual_ms,
            "checkpoint {}: virtual_ms mismatch",
            n
        );
        assert_eq!(
            got.gametic, expected.gametic,
            "checkpoint {}: gametic mismatch",
            n
        );
        assert_eq!(
            got.gamestate, expected.gamestate,
            "checkpoint {}: gamestate mismatch",
            n
        );
        assert_eq!(
            got.rndindex, expected.rndindex,
            "checkpoint {}: rndindex mismatch",
            n
        );
        assert_eq!(
            got.prndindex, expected.prndindex,
            "checkpoint {}: prndindex mismatch",
            n
        );
        assert_eq!(
            got.health, expected.health,
            "checkpoint {}: health mismatch",
            n
        );
        assert_eq!(
            got.armorpoints, expected.armorpoints,
            "checkpoint {}: armorpoints mismatch",
            n
        );
        assert_eq!(
            got.killcount, expected.killcount,
            "checkpoint {}: killcount mismatch",
            n
        );
        assert_eq!(
            got.itemcount, expected.itemcount,
            "checkpoint {}: itemcount mismatch",
            n
        );
        assert_eq!(
            got.secretcount, expected.secretcount,
            "checkpoint {}: secretcount mismatch",
            n
        );
        assert_eq!(
            got.readyweapon, expected.readyweapon,
            "checkpoint {}: readyweapon mismatch",
            n
        );
        assert_eq!(got.ammo, expected.ammo, "checkpoint {}: ammo mismatch", n);
        assert_eq!(got.mo_x, expected.mo_x, "checkpoint {}: mo_x mismatch", n);
        assert_eq!(got.mo_y, expected.mo_y, "checkpoint {}: mo_y mismatch", n);
        assert_eq!(got.mo_z, expected.mo_z, "checkpoint {}: mo_z mismatch", n);
        assert_eq!(
            got.mo_angle, expected.mo_angle,
            "checkpoint {}: mo_angle mismatch",
            n
        );
    }

    let bless = std::env::var("BLESS").is_ok();

    // Drive the engine for TOTAL_TICS ticks.
    eprintln!(
        "demo_playthrough: {} checkpoints, {} total tics",
        CHECKPOINTS.len(),
        TOTAL_TICS
    );
    for tic in 0..TOTAL_TICS {
        unsafe {
            doomgeneric_sys::doomgeneric_Tick();
        }

        if is_checkpoint(tic + 1) {
            let checkpoint_idx = snapshots.len();
            let pct = ((checkpoint_idx + 1) * 100) / CHECKPOINTS.len();
            unsafe {
                eprintln!(
                    "  checkpoint {}/{} ({}%) at tic {} gametic={} leveltime={}",
                    checkpoint_idx + 1,
                    CHECKPOINTS.len(),
                    pct,
                    tic + 1,
                    gametic,
                    room::doom::p_tick::leveltime,
                );
            }
            let snap = capture_snapshot(tic + 1);
            if !bless {
                assert!(
                    checkpoint_idx < BASELINE.len(),
                    "checkpoint {}: no baseline entry (got {} snapshots, baseline has {})",
                    checkpoint_idx,
                    checkpoint_idx + 1,
                    BASELINE.len(),
                );
                validate_snapshot(checkpoint_idx, &snap, &BASELINE[checkpoint_idx]);
            }
            snapshots.push(snap);
        }
    }

    if bless {
        println!("/* BLESS output — paste into BASELINE */");
        println!("const BASELINE: &[Snapshot] = &[");
        for snap in &snapshots {
            println!("    Snapshot {{");
            println!("        tic: {},", snap.tic);
            println!("        virtual_ms: {},", snap.virtual_ms);
            println!("        frames: {},", snap.frames);
            println!("        gametic: {},", snap.gametic);
            println!("        gamestate: {},", snap.gamestate);
            println!("        rndindex: {},", snap.rndindex);
            println!("        prndindex: {},", snap.prndindex);
            println!("        health: {},", snap.health);
            println!("        armorpoints: {},", snap.armorpoints);
            println!("        killcount: {},", snap.killcount);
            println!("        itemcount: {},", snap.itemcount);
            println!("        secretcount: {},", snap.secretcount);
            println!("        readyweapon: {},", snap.readyweapon);
            println!("        ammo: {:?},", snap.ammo);
            println!("        mo_x: {},", snap.mo_x);
            println!("        mo_y: {},", snap.mo_y);
            println!("        mo_z: {},", snap.mo_z);
            println!("        mo_angle: {},", snap.mo_angle);
            println!("    }},");
        }
        println!("];");
        panic!("BLESS mode: paste the output above into BASELINE, then re-run without BLESS=1");
    } else {
        assert_eq!(
            snapshots.len(),
            BASELINE.len(),
            "snapshot count mismatch: got {}, expected {}",
            snapshots.len(),
            BASELINE.len()
        );
        for (i, (got, expected)) in snapshots.iter().zip(BASELINE.iter()).enumerate() {
            assert_eq!(got.tic, expected.tic, "checkpoint {}: tic mismatch", i);
            assert_eq!(
                got.virtual_ms, expected.virtual_ms,
                "checkpoint {}: virtual_ms mismatch",
                i
            );
            assert_eq!(
                got.gametic, expected.gametic,
                "checkpoint {}: gametic mismatch",
                i
            );
            assert_eq!(
                got.gamestate, expected.gamestate,
                "checkpoint {}: gamestate mismatch",
                i
            );
            assert_eq!(
                got.rndindex, expected.rndindex,
                "checkpoint {}: rndindex mismatch",
                i
            );
            assert_eq!(
                got.prndindex, expected.prndindex,
                "checkpoint {}: prndindex mismatch",
                i
            );
            assert_eq!(
                got.health, expected.health,
                "checkpoint {}: health mismatch",
                i
            );
            assert_eq!(
                got.armorpoints, expected.armorpoints,
                "checkpoint {}: armorpoints mismatch",
                i
            );
            assert_eq!(
                got.killcount, expected.killcount,
                "checkpoint {}: killcount mismatch",
                i
            );
            assert_eq!(
                got.itemcount, expected.itemcount,
                "checkpoint {}: itemcount mismatch",
                i
            );
            assert_eq!(
                got.secretcount, expected.secretcount,
                "checkpoint {}: secretcount mismatch",
                i
            );
            assert_eq!(
                got.readyweapon, expected.readyweapon,
                "checkpoint {}: readyweapon mismatch",
                i
            );
            assert_eq!(got.ammo, expected.ammo, "checkpoint {}: ammo mismatch", i);
            assert_eq!(got.mo_x, expected.mo_x, "checkpoint {}: mo_x mismatch", i);
            assert_eq!(got.mo_y, expected.mo_y, "checkpoint {}: mo_y mismatch", i);
            assert_eq!(got.mo_z, expected.mo_z, "checkpoint {}: mo_z mismatch", i);
            assert_eq!(
                got.mo_angle, expected.mo_angle,
                "checkpoint {}: mo_angle mismatch",
                i
            );
        }
    }
}
