What's needed: quick-n-dirty higher-half mappings setup and kernel physical memory view setup (both in TTBR1), identity mapping for init_thread (in TTBR0).

- `__kernel_start` to `__kernel_end` map at KERNEL_HIGH_BASE
- 0 to phys_ram_size (from DTB) map at KERNEL_PHYS_WINDOW
- `__init_thread_start` till `__init_thread_end` identity-map (no code/data split yet?)

Steps:
- Buildable
- Run steps by step
  - Enter kernel init in EL2 - this will be needed to set up kernel mappings
  - Print DTB
  - Print max RAM from DTB
  - Print kernel covered area
  - Print KERNEL_HIGH_BASE
  - Print kernel mappings size and attribs
  - Print init_thread covered area
  - Print init_thread mappings size
