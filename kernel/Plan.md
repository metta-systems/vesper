What's needed: quick-n-dirty higher-half mappings setup and kernel physical memory view setup (both in TTBR1), identity mapping for init_thread (in TTBR0).

- `__kernel_start` to `__kernel_end` map at KERNEL_HIGH_BASE
- 0 to phys_ram_size (from DTB) map at KERNEL_PHYS_WINDOW
- `__init_thread_start` till `__init_thread_end` identity-map (no code/data split yet?)

Steps:
- [x] Buildable
- [x] build all shit together into a binary
- [x] make separate init_thread section and start booting from init_thread
- [x] make early_print work
- [x] parse dtb
- [x] Print that we can invoke kernel function using a syscall from the init_thread (even it if runs at the same EL for now)
- [x] Enter kernel init in EL2 - this will be needed to set up kernel mappings
- [x] Print DTB
- [x] Print max RAM from DTB

- [x] Allocate a single key slot in a global domain struct
- [x] Fill it with capability to DebugConsole
- [x] Invoke DebugConsole.Write via syscall

- [ ] Make some caps work - Untypeds, Domains, Buffers, what else?
- [ ] Test out syscalls from EL0

- [ ] Print kernel covered area
- [ ] Print KERNEL_HIGH_BASE
- [ ] Print kernel mappings size and attribs
- [ ] Print init_thread covered area
- [ ] Print init_thread mappings size
- START FILLING IN CAPS
  - [ ] untypeds
  - [ ] init_thread context and domain

Whatever kernel links must also be located in high-mem mapping, so we cannot share this code with init_thread at all!
This means it's probably sensible to build kernel as a separate ELF file linked entirely high, then merge it with the init_thread binary through specially-named sections; there should be no symbol resolution across two binaries, so the nucleus image is solely pulled via it's PHDRS (but we need to place the BSS which will be erased by the init_thread before turning the MMU on)

- [x] See gh:metta-systems/kernel-embed-prototype for an outline of this approach - copy it here and lets go.
