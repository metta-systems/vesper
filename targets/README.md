@todo - factor it out into separate repo
@todo - "panic-strategy": "abort" is ok for baremetal, but not for -metta, right?

# vesper-targets

These are [target
specifications](https://github.com/rust-lang/rfcs/blob/master/text/0131-target-specification.md)
suitable for cross-compiling Rust crates for Vesper. Set your `RUST_TARGET_PATH` to point to this directory.

These are very much based on Robigalia's [sel4-targets](https://gitlab.com/robigalia/sel4-targets).

## Status

Complete for aarch64. Untested for anything else.

## Generating target specifications:

See [description in rust docs](https://doc.rust-lang.org/rustc/targets/custom.html).

To generate a target specification json template, run

```
rustc +nightly -Z unstable-options --target=wasm32-unknown-unknown --print target-spec-json
```
