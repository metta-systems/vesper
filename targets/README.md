# vesper-targets

These are [target
specifications](https://github.com/rust-lang/rfcs/blob/master/text/0131-target-specification.md)
suitable for cross-compiling Rust crates for Vesper.

They are very much based on Robigalia's [sel4-targets](https://gitlab.com/robigalia/sel4-targets).

- `aarch64-metta-none-eabi`: soft-float, no FP/SIMD. The kernel and every integer-only
  component.
- `aarch64-metta-none-hardfloat`: hard-float with NEON. Components whose AddressSpace allows
  FP/SIMD (the Justfile's `fp_components`). The two ABIs do not link together, so shared
  libraries are built for each.

## Status

Complete for aarch64. Untested for anything else.

## Generating target specifications:

See [description in rust docs](https://doc.rust-lang.org/rustc/targets/custom.html).

To generate a target specification json template, run

```
rustc +nightly -Z unstable-options --target=<your target name> --print target-spec-json
```

### Erratums

- [fix-cortex-a53-843419](https://reviews.llvm.org/D36749) implemented in LLD

### To do

"panic-strategy": "abort" is ok for baremetal targets, but not for -metta, right? Will rework for userspace targets when we have unwinding.



----

For more information please re-read.
