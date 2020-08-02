# vesper-targets

These are [target
specifications](https://github.com/rust-lang/rfcs/blob/master/text/0131-target-specification.md)
suitable for cross-compiling Rust crates for Vesper.

They are very much based on Robigalia's [sel4-targets](https://gitlab.com/robigalia/sel4-targets).

## Status

Complete for aarch64. Untested for anything else.

## Generating target specifications:

See [description in rust docs](https://doc.rust-lang.org/rustc/targets/custom.html).

To generate a target specification json template, run

```
rustc +nightly -Z unstable-options --target=<your target name> --print target-spec-json
```

### To do

"panic-strategy": "abort" is ok for baremetal targets, but not for -metta, right? Will rework for userspace targets when we have unwinding.
