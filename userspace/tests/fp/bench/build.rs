//! Extract E1M1's vertices from the shareware `doom1.wad` (Wood's copy) into
//! `vertices.rs`: two `f32` arrays, padded to a whole number of chunks with
//! points behind every viewpoint.
//!
//! A WAD starts with `IWAD`, the lump count and the directory offset (little
//! endian `i32`s); each 16-byte directory entry is the lump's offset, its size
//! and its 8-byte NUL-padded name. A map is a marker lump (`E1M1`) followed by
//! its lumps; `VERTEXES` holds `i16` x, y pairs in map units.

use std::{env, fs, path::Path};

/// Vertices per chunk; must match `CHUNK` in `src/main.rs`.
const CHUNK: usize = 16;
/// Viewpoints the frames cycle through.
const VIEWS_COUNT: u16 = 64;

fn word(bytes: &[u8], at: usize) -> usize {
    let value = i32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
    usize::try_from(value).expect("a WAD offset or size is not negative")
}

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let wad_path = Path::new(&manifest).join("../../../wood/wad/doom1.wad");
    println!("cargo::rerun-if-changed={}", wad_path.display());
    println!("cargo::rerun-if-changed=build.rs");
    let wad = fs::read(&wad_path).unwrap_or_else(|error| panic!("{}: {error}", wad_path.display()));
    assert_eq!(&wad[..4], b"IWAD", "not an IWAD");
    let (count, directory) = (word(&wad, 4), word(&wad, 8));
    let names: Vec<(&[u8], usize, usize)> = (0..count)
        .map(|index| {
            let entry = directory + index * 16;
            let name = &wad[entry + 8..entry + 16];
            let length = name.iter().position(|&byte| byte == 0).unwrap_or(8);
            (&name[..length], word(&wad, entry), word(&wad, entry + 4))
        })
        .collect();
    let map = names
        .iter()
        .position(|(name, ..)| *name == b"E1M1")
        .expect("E1M1 in doom1.wad");
    let &(_, offset, size) = names[map + 1..]
        .iter()
        .find(|(name, ..)| *name == b"VERTEXES")
        .expect("E1M1's VERTEXES");
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for &[x_low, x_high, y_low, y_high] in wad[offset..offset + size].as_chunks::<4>().0 {
        xs.push(i16::from_le_bytes([x_low, x_high]));
        ys.push(i16::from_le_bytes([y_low, y_high]));
    }
    let real = xs.len();
    // Padding: the first vertex again, which keeps the work the same.
    while xs.len() % CHUNK != 0 {
        xs.push(xs[0]);
        ys.push(ys[0]);
    }
    let list = |values: &[i16]| {
        values
            .iter()
            .map(|value| format!("{value}.0"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    // The viewpoints: a step off every seventh vertex, each looking a
    // different way around the circle (no `sin` or `cos` in `core`).
    let views = (0..VIEWS_COUNT)
        .map(|index| {
            let at = usize::from(index) * 7 % real;
            let angle = std::f64::consts::TAU * f64::from(index) / f64::from(VIEWS_COUNT);
            format!(
                "View {{ x: {:.1}, y: {:.1}, cos: {:.6}, sin: {:.6} }}",
                f64::from(xs[at]) + 16.0,
                f64::from(ys[at]) + 16.0,
                angle.cos(),
                angle.sin(),
            )
        })
        .collect::<Vec<_>>()
        .join(",\n    ");
    let source = format!(
        "// E1M1's {real} vertices in `doom1.wad`, padded to whole chunks.\n\
         /// Vertex x coordinates, padded to whole chunks.\n\
         pub static XS: [f32; {padded}] = [{xs}];\n\
         /// Vertex y coordinates, padded to whole chunks.\n\
         pub static YS: [f32; {padded}] = [{ys}];\n\
         /// Where the frames look from.\n\
         pub static VIEWS: [View; {VIEWS_COUNT}] = [\n    {views}\n];\n",
        padded = xs.len(),
        xs = list(&xs),
        ys = list(&ys),
    );
    let out = Path::new(&env::var("OUT_DIR").expect("OUT_DIR")).join("vertices.rs");
    fs::write(out, source).expect("write vertices.rs");
}
