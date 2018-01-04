# Vesper

## About kernel

Vesper is a capability-based single-address-space exokernel, it tries to remain small and secure. To achieve this, kernel functionality is extremely limited - it provides only address space isolation and IPC, after bootup kernel does not allocate any memory itself.

Exokernel's distinctive trait is that it provides mechanisms but not policies. Vesper tries to move as many policy decisions as possible to the library OS.

* Single-address-space is a mechanism for providing pointer transparency between processes. Sharing a buffer is nearly as simple as passing out its address. Even though single-address-space is not a basic requirement and may be seen as an imposed policy decision, it does provide mechanism for efficient data passing between protection domains. This is important for modern media-rich communications.

* IPC is a mechanism providing secure interaction between processes.

* Capabilities are a mechanism providing access rights control and universal authority delegation for OS objects.

* Interrupts come from hardware, usually in privileged mode and kernel is responsible for translating them into invocations of the device drivers' handlers.

### Scheduling

Scheduling can be viewed as the process of multiplexing the CPU resource between computational tasks. The schedulable entity of an operating system often places constraints both on the scheduling algorithms which may be employed and the functionality provided to the application. The recent gain in popularity of multi-threaded programming due to languages such as Modula-3 [Nelson 91] has led many operating system designers to provide kernel-level thread support mechanisms [Accetta 86, Rozier 90]. The kernel therefore schedules threads rather than processes. Whilst this reduces the functionality required in applications and usually results in more efficient processor context-switches, the necessary thread scheduling policy decisions must also be migrated into the kernel. As pointed out in [Barham 96], this is highly undesirable.

The desire to move such decisions out of the kernel make interesting variants where actual scheduling is performed by the user-level domain scheduler upon an **upcall** from the kernel. TBD

## Real Time

At the moment this is not a real-time kernel. It has a small number of potentially long-running kernel operations that are not preemptable (e.g., endpoint deletion and recycling, scheduling, frame and CNode initialisation). This may change in future versions.

## Credits

Vesper has been influenced by the kernels in L4 family, notably seL4. Fawn and Nemesis provided inspiration for single-address-space and vertical integration of the applications.

## Build instructions

Use rustc nightly 2018-04-01 or later because of [bugs fixed](https://github.com/rust-lang/rust/issues/48884).

```
cargo xbuild --target=targets/aarch64-vesper-metta.json --release

# Post-command:
sh .cargo/runscript.sh
cp target/aarch64-vesper-metta/release/vesper.bin /Volumes/boot/vesper

# config.txt on RPi3
kernel=vesper
arm_64bit=1

# To run in qemu, `brew install qemu --HEAD --with-libusb` and
### This command is not supported by cargo-xbuild yet: xargo run --target=aarch64-vesper-metta
# Use this instead:
sh .cargo/runscript.sh
```

## OSdev help

Based on [Raspi3 tutorials by Andre Richter](https://github.com/rust-embedded/rust-raspi3-tutorial/blob/master/05_uart0/src/uart.rs),
which are in turn based on [Raspi3 tutorials by bzt](https://github.com/bztsrc/raspi3-tutorial/).
Various references from [OSDev Wiki](https://wiki.osdev.org/Raspberry_Pi_Bare_Bones) and [RaspberryPi.org manuals](https://www.raspberrypi.org/app/uploads/2012/02/BCM2835-ARM-Peripherals.pdf).
