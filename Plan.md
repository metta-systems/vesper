- [ ] ObjectPool .meta is limited to 256 entires, which is wrong - the meta should be allocated together with the pool from the Untyped, this is where we know the actual capacity.
- [ ] seemingly AI decided we use process stacks, while it should be interrupt stacks for kernel state. Domain context should be stored in the DCB.
- [ ] Extract testing bits from kickstart into a separate kicktest binary.
- [ ] pub asid: Option<u16>, should probably use Option<NonZero<u16>> in domain.rs
  - [ ] similarly for some other int-based optionals, where 0 is clearly an invalid value - find them

- [ ] two mmio tests ignored -- running 2 tests
  test libs/mmio/src/lib.rs - MMIODerefWrapper<T> (line 42) ... ignored
  test libs/mmio/src/lib.rs - MMIODerefWrapper<T> (line 46) ... ignored

- [ ] "cannot parse the debug map for" - where it is from, toolchain or my scripts?
    info: Vector table:        0xFFFF800000003800 (size: 0x800)
  error: cannot parse the debug map for '/Users/berkus/Projects/Metta/vesper/target/aarch64-metta-none-eabi/release/build/kickstart/05af02c1fa98982b/out/kickstart': The file was not recognized as a valid object file

- [/] What's needed: quick-n-dirty higher-half mappings setup and kernel physical memory view setup (both in TTBR1), identity mapping for kickstart (in TTBR0).
  - `__kernel_start` to `__kernel_end` map at KERNEL_HIGH_BASE
  - 0 to phys_ram_size (from DTB) map at KERNEL_PHYS_WINDOW
  - `__kickstart_start` till `__kickstart_end` identity-map (no code/data split yet?)

Steps:
- [ ] Move tests from kernel/tests to individual libs?

- [x] Make some caps work - Untypeds, Domains, Buffers, what else?
- [x] Test out syscalls from EL0

- START FILLING IN CAPS
  - [ ] untypeds
  - [ ] kickstart context and domain

---

# Completed

- [x] Buildable
- [x] build all shit together into a binary
- [x] make separate kickstart section and start booting from kickstart
- [x] make early_print work
- [x] parse dtb
- [x] Print that we can invoke kernel function using a syscall from the kickstart (even it if runs at the same EL for now)
- [x] Enter kernel init in EL2 - this will be needed to set up kernel mappings
- [x] Print DTB
- [x] Print max RAM from DTB

- [x] Allocate a single key slot in a global domain struct
- [x] Fill it with capability to DebugConsole
- [x] Invoke DebugConsole.Write via syscall

- [x] Generate complete memory map from DTB, print it out

- [x] Print kernel covered area
- [x] Print KERNEL_HIGH_BASE
- [x] Print kernel mappings size and attribs
- [x] Print kickstart covered area
- [x] Print kickstart mappings size



Whatever kernel links must also be located in high-mem mapping, so we cannot share this code with kickstart at all!
This means it's probably sensible to build kernel as a separate ELF file linked entirely high, then merge it with the kickstart binary through specially-named sections; there should be no symbol resolution across two binaries, so the nucleus image is solely pulled via it's PHDRS (but we need to place the BSS which will be erased by the kickstart before turning the MMU on)

- [x] See gh:metta-systems/kernel-embed-prototype for an outline of this approach - copy it here and lets go.
