What's needed: quick-n-dirty higher-half mappings setup and kernel physical memory view setup (both in TTBR1), identity mapping for init_thread (in TTBR0).

- `__kernel_start` to `__kernel_end` map at KERNEL_HIGH_BASE
- 0 to phys_ram_size (from DTB) map at KERNEL_PHYS_WINDOW
- `__init_thread_start` till `__init_thread_end` identity-map (no code/data split yet?)
