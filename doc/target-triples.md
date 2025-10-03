Many targets already have a known triple used to describe them, typically in the form `ARCH-VENDOR-SYS-ABI`. You should aim to use the same triple that LLVM uses; however, it may differ if you need to specify additional information to Rust that LLVM does not know about. Although the triple is technically only for human use, it's important for it to be unique and descriptive especially if the target will be upstreamed in the future.

The ARCH part is typically just the architecture name, except in the case of 32-bit ARM. For example, you would probably use x86_64 for those processors, but specify the exact ARM architecture version. Typical values might be armv7, armv5te, or thumbv7neon. Take a look at the names of the built-in targets for inspiration. `aarch64`

The VENDOR part is optional and describes the manufacturer. Omitting this field is the same as using unknown. `metta`

The SYS part describes the OS that is used. Typical values include win32, linux, and darwin for desktop platforms. none is used for bare-metal usage. `none` -- use a codename instead of Metta here, something metta-related maybe?

The ABI part describes how the process starts up. eabi is used for bare metal, while gnu is used for glibc, musl for musl, etc. `eabi`

Now that you have a target triple, create a file with the name of the triple and a .json extension. For example, a file describing armv7a-none-eabi would have the filename armv7a-none-eabi.json