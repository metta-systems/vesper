//! Diagnostic binary that prints the size and field offsets of the most
//! ABI-sensitive map-data structs.
//!
//! The Doom map format and the C engine both rely on `#[repr(C)]` layouts
//! matching exactly between the runtime types and the compiled-in tables (and
//! across the FFI boundary with any residual C code).  Running this binary
//! gives a quick visual diff against the expected layout when chasing
//! save-game corruption, render glitches, or BSP-traversal bugs introduced by
//! struct edits.
//!
//! Build and run with:
//!
//! ```sh
//! cargo run --release --bin struct_sizes
//! ```
//!
//! No options, no output parsing: it just prints to stdout.

/// Program entry point: dump struct sizes and field offsets to stdout.
///
/// Each block of `println!` calls follows the same pattern: one heading line,
/// then one line per field, formatted to line up visually in a fixed-width
/// terminal.  Add new types here as they become layout-sensitive.
fn main() {
    use room::doom::c_ffi::*;
    use std::mem::{offset_of, size_of};

    println!("Rust struct sizes:");
    println!("  vertex_t:     {}", size_of::<vertex_t>());
    println!("  seg_t:        {}", size_of::<seg_t>());
    println!("  subsector_t:  {}", size_of::<subsector_t>());
    println!("  sector_t:     {}", size_of::<sector_t>());
    println!("  line_t:       {}", size_of::<line_t>());
    println!("  side_t:       {}", size_of::<side_t>());
    println!("  node_t:       {}", size_of::<node_t>());

    println!("\nseg_t field offsets:");
    println!("  v1:           {}", offset_of!(seg_t, v1));
    println!("  v2:           {}", offset_of!(seg_t, v2));
    println!("  angle:        {}", offset_of!(seg_t, angle));
    println!("  offset:       {}", offset_of!(seg_t, offset));
    println!("  linedef:      {}", offset_of!(seg_t, linedef));
    println!("  sidedef:      {}", offset_of!(seg_t, sidedef));
    println!("  frontsector:  {}", offset_of!(seg_t, frontsector));
    println!("  backsector:   {}", offset_of!(seg_t, backsector));

    println!("\nsubsector_t field offsets:");
    println!("  sector:       {}", offset_of!(subsector_t, sector));
    println!("  numlines:     {}", offset_of!(subsector_t, numlines));
    println!("  firstline:    {}", offset_of!(subsector_t, firstline));

    println!("\nsector_t field offsets:");
    println!("  floorheight:  {}", offset_of!(sector_t, floorheight));
    println!("  ceilingheight:{}", offset_of!(sector_t, ceilingheight));
    println!("  floorpic:     {}", offset_of!(sector_t, floorpic));
    println!("  ceilingpic:   {}", offset_of!(sector_t, ceilingpic));
    println!("  lightlevel:   {}", offset_of!(sector_t, lightlevel));
    println!("  special:      {}", offset_of!(sector_t, special));
    println!("  tag:          {}", offset_of!(sector_t, tag));
    println!("  soundtraversed:{}", offset_of!(sector_t, soundtraversed));
    println!("  soundtarget:  {}", offset_of!(sector_t, soundtarget));
    println!("  blockbox:     {}", offset_of!(sector_t, blockbox));
    println!("  soundorg:     {}", offset_of!(sector_t, soundorg));
    println!("  validcount:   {}", offset_of!(sector_t, validcount));
    println!("  thinglist:    {}", offset_of!(sector_t, thinglist));
    println!("  specialdata:  {}", offset_of!(sector_t, specialdata));
    println!("  linecount:    {}", offset_of!(sector_t, linecount));
    println!("  lines:        {}", offset_of!(sector_t, lines));

    println!("\nside_t field offsets:");
    println!("  textureoffset:{}", offset_of!(side_t, textureoffset));
    println!("  rowoffset:    {}", offset_of!(side_t, rowoffset));
    println!("  toptexture:   {}", offset_of!(side_t, toptexture));
    println!("  bottomtexture:{}", offset_of!(side_t, bottomtexture));
    println!("  midtexture:   {}", offset_of!(side_t, midtexture));
    println!("  sector:       {}", offset_of!(side_t, sector));

    println!("\nline_t field offsets:");
    println!("  v1:           {}", offset_of!(line_t, v1));
    println!("  v2:           {}", offset_of!(line_t, v2));
    println!("  dx:           {}", offset_of!(line_t, dx));
    println!("  dy:           {}", offset_of!(line_t, dy));
    println!("  flags:        {}", offset_of!(line_t, flags));
    println!("  special:      {}", offset_of!(line_t, special));
    println!("  tag:          {}", offset_of!(line_t, tag));
    println!("  sidenum:      {}", offset_of!(line_t, sidenum));
    println!("  bbox:         {}", offset_of!(line_t, bbox));
    println!("  slopetype:    {}", offset_of!(line_t, slopetype));
    println!("  frontsector:  {}", offset_of!(line_t, frontsector));
    println!("  backsector:   {}", offset_of!(line_t, backsector));
    println!("  validcount:   {}", offset_of!(line_t, validcount));
    println!("  specialdata:  {}", offset_of!(line_t, specialdata));
}
