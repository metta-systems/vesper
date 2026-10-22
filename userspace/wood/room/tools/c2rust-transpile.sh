#!/usr/bin/env bash
# Helper script to regenerate the c2rust-intermediate crate from vendored C sources.
#
# Usage:
#   ./tools/c2rust-transpile.sh
#
# Prerequisites:
#   - c2rust is installed (cargo install c2rust)
#   - jq is installed
#   - Run from the workspace root (/chaos/dev/mine/rust/room)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
VENDOR_DIR="${ROOT_DIR}/vendor/doomgeneric"
OUTPUT_DIR="${ROOT_DIR}/c2rust-intermediate"
CC_JSON="${ROOT_DIR}/compile_commands.json"

# Files to exclude from transpilation (see plan for rationale)
EXCLUDES=(
    "layout_probe.c"
    "gusconf.c"
    "m_misc.c"          # contains variadic macros (M_StringJoin, M_vsnprintf) that crash c2rust
    "dummy.c"
    "doomdef.c"
)

echo "=== Step 1: Generating compile_commands.json ==="

# Build the JSON array
ENTRIES=()
for cfile in "${VENDOR_DIR}"/*.c; do
    basename_cfile="$(basename "$cfile")"

    # Skip excluded files
    skip=false
    for ex in "${EXCLUDES[@]}"; do
        if [[ "$basename_cfile" == "$ex" ]]; then
            skip=true
            break
        fi
    done
    if $skip; then
        echo "  SKIP  $basename_cfile"
        continue
    fi

    # Relative path from workspace root
    relfile="vendor/doomgeneric/${basename_cfile}"
    objname="${basename_cfile%.c}.o"

    # Build the JSON entry.  Note: -c and -o are required by c2rust.
    cmd="cc -DNORMALUNIX -DLINUX -D_DEFAULT_SOURCE -Ivendor/doomgeneric -c ${relfile} -o ${objname}"

    ENTRY=$(jq -n \
        --arg dir "$ROOT_DIR" \
        --arg cmd "$cmd" \
        --arg file "$relfile" \
        '{directory: $dir, command: $cmd, file: $file}')
    ENTRIES+=("$ENTRY")
done

# Write the JSON array
printf '%s\n' "${ENTRIES[@]}" | jq -s '.' > "$CC_JSON"
echo "  Wrote ${#ENTRIES[@]} entries to ${CC_JSON}"

echo ""
echo "=== Step 2: Running c2rust transpile ==="

# Ensure output directory exists and is empty
rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

cd "$ROOT_DIR"
c2rust transpile "$CC_JSON" \
    --emit-build-files \
    --overwrite-existing \
    --output-dir "$OUTPUT_DIR"

echo ""
echo "=== Step 3: Fixing up generated crate ==="

# c2rust --emit-build-files creates src/ and Cargo.toml in OUTPUT_DIR,
# but lib.rs ends up in OUTPUT_DIR root instead of src/.

# Move lib.rs into src/
if [ -f "${OUTPUT_DIR}/lib.rs" ]; then
    mv "${OUTPUT_DIR}/lib.rs" "${OUTPUT_DIR}/src/lib.rs"
    echo "  Moved lib.rs -> src/lib.rs"
fi

# Overwrite Cargo.toml with proper metadata and dependencies
CARGO_TOML="${OUTPUT_DIR}/Cargo.toml"
cat > "$CARGO_TOML" <<EOF
[package]
name = "c2rust-intermediate"
version = "0.1.0"
edition = "2021"

# This crate is a pure reference / debugging aid.  It is NOT linked into the
# main binary.  Do not attempt to resolve symbol conflicts with the hand-ported
# code — they are expected to coexist only as a behavioural reference.

[dependencies]
c2rust-bitfields = "0.19"
libc = "0.2"

[lib]
name = "c2rust_intermediate"
path = "src/lib.rs"
EOF
echo "  Updated ${CARGO_TOML}"

# c2rust generates modules wrapped in `pub mod src { ... }` which expects files
# under src/src/.  Rewrite lib.rs to declare modules directly.
LIB_RS="${OUTPUT_DIR}/src/lib.rs"
MODULES=$(cd "${OUTPUT_DIR}/src" && for f in *.rs; do
    [ "$f" = "lib.rs" ] && continue
    echo "pub mod ${f%.rs};"
done)

cat > "$LIB_RS" <<'RUST'
#![allow(
    clippy::missing_safety_doc,
    dead_code,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unused_assignments,
    unused_mut,
    improper_ctypes,
    unused_imports,
    unused_variables
)]
#![feature(extern_types)]

#[macro_use]
extern crate c2rust_bitfields;
extern crate libc;

RUST

printf '%s\n' "$MODULES" >> "$LIB_RS"
echo "  Rewrote src/lib.rs with direct module declarations"

# Create rust-toolchain.toml pointing to nightly (extern_types is unstable)
RUST_TOOLCHAIN="${OUTPUT_DIR}/rust-toolchain.toml"
cat > "$RUST_TOOLCHAIN" <<EOF
[toolchain]
channel = "nightly"
components = ["rustfmt"]
EOF
echo "  Wrote rust-toolchain.toml (nightly required for extern_types)"

echo ""
echo "=== Done ==="
echo "Output: ${OUTPUT_DIR}"
echo ""
echo "The crate has been added to workspace members in the root Cargo.toml"
echo "but is excluded from default-members so it does not build by default."
echo ""
echo "To check the reference crate:"
echo "  cargo +nightly check -p c2rust-intermediate"
echo ""
echo "Known caveats:"
echo "  - Requires nightly Rust (extern_types feature)"
echo "  - Produces many warnings; these are suppressed via #![allow(...)]"
echo "  - Duplicate type definitions across modules are expected"
