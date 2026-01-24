What's needed: quick-n-dirty higher-half mappings setup and kernel physical memory view setup (both in TTBR1), identity mapping for init_thread (in TTBR0).

- `__kernel_start` to `__kernel_end` map at KERNEL_HIGH_BASE
- 0 to phys_ram_size (from DTB) map at KERNEL_PHYS_WINDOW
- `__init_thread_start` till `__init_thread_end` identity-map (no code/data split yet?)

Steps:
- Buildable
- Print that we can invoke kernel function using a syscall from the init_thread (even it if runs at the same EL for now)

- Run steps by step
  - Enter kernel init in EL2 - this will be needed to set up kernel mappings
  - Print DTB
  - Print max RAM from DTB
  - Print kernel covered area
  - Print KERNEL_HIGH_BASE
  - Print kernel mappings size and attribs
  - Print init_thread covered area
  - Print init_thread mappings size



Whatever kernel links must also be located in high-mem mapping, so we cannot share this code with init_thread at all!
This means it's probably sensible to build kernel as a separate ELF file linked entirely high, then merge it with the init_thread binary through specially-named sections; there should be no symbol resolution across two binaries, so the nucleus image is solely pulled via it's PHDRS (but we need to place the BSS which will be erased by the init_thread before turning the MMU on)

See gh:metta-systems/kernel-embed-prototype for an outline of this approach - copy it here and lets go.
