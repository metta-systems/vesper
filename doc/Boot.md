# The Kernel Boot Sequence

NOTE: This section focuses on the ARM architecture.

After the board is set up with the bootloader and execution is handed off to the elfloader, where it then branches to the kernel. The kernel boot sequence can be traced in `src/arch/arm/kernel/boot.c`. The first function to be called is `init_kernel`. Then `try_init_kernel` is called and depending on the number of CPUs, this function branches to `try_init_kernel_secondary_core`.

In `try_init_kernel` several important things happen:

-    Virtual memory for the kernel is set up in `map_kernel_window`.
     -   This also calls `map_kernel_devices`
         -   Where the platform dependent `kernel_devices` gets mapped
         -   This is 32/64 bit dependent and in the `src/arch/arm/$ARCH/machine/hardware.c` source file.
-    CPU is initialized in `init_cpu`
     -   The `activate_global_pd` is a macro that is dependent on whether it is running in 32 or 64 bit mode
         -   In 64 bit mode this actually calls `activate_kernel_vspace`
             -   This function is in `src/arch/arm/64/kernel/vspace.c`
     -   The kernel stack is set up
     -   The timer is initialized
-    Platform is initialized in `init_plat`
     -   The IRQ Controller is initialized with `initIRQController`
         -   The implementation of this is found in `src/plat/$PLAT/machine/` or `src/arch/arm/machine/gic_$GIC_VERSION.c`
     -   The L2 Cache is initialized
     -   In the future other platform dependent initializations may take place here
-    Capabilities are created for the root environment
-    The boot info frame is allocated and initialized
-    The initial address space is created
-    The boot info frame capability is created
-    The initial threads IPC buffer is created
-    This is all brought together and the initial thread control block of type `tcb_t` is created with a call to `create_initial_thread`
-    L1 cache is invalidated
-    Execution is returned to the `init_kernel` function

Once execution returns to the `init_kernel` function, the thread is scheduled and execution branches to the root user space thread.
