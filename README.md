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

Use rustc nightly 2020-07-15 with cargo nightly of the same or later date.

Install tools: `cargo install just cargo-make`.
Install qemu (at least version 4.1.1): `brew install qemu`.

To build kernel and run it in QEMU emulator:

```
just qemu
```

To build kernel for Raspberry Pi and copy it to SDCard mounted at `/Volumes/BOOT/`:

```
just device
```

On the device boot SD card you'll need a configuration file instructing RasPi to launch in 64-bit mode.

```
# config.txt on RPi3
arm_64bit=1
```

## Development flow

`mainline`, `develop` and `released` branches:

- `feature branches` are fluid development lines which may be discarded or merged into `develop`. Feature branches must be either merged or fast-forward merged ("landed") into develop. Squashing history during merge is not permitted - commits must be sorted and squashed as necessary before merge.
- `develop` is currenly developed changes. History is recommended to be immutable, however mutations are possible in some cases. Feature branches are merged into develop for stabilisation, then develop is merged into the mainline. `Develop` must be either merged or fast-forward merged ("landed") into `mainline`. Squashing history during merge is not permitted - commits must be sorted as necessary before merge. Avoid direct commits to develop. It is recommended to perform stabilisation fixes in a separate branch and then landing it into develop.
- `mainline` is for generally accepted changes. History is immutable, to record reversals make a revert commit with explanations why. Changes from `develop` are merged or landed into the mainline after stabilisation.
- `released` branch records points from mainline which were officially released. Mutations are not possible. Only non-fast-forward merges from mainline are acceptable. Releases are marked as annotated tags on this branch.

## OSdev help

Based on [Raspi3 tutorials by Andre Richter](https://github.com/rust-embedded/rust-raspi3-tutorial/blob/master/05_uart0/src/uart.rs),
which are in turn based on [Raspi3 tutorials by bzt](https://github.com/bztsrc/raspi3-tutorial/).
Various references from [OSDev Wiki](https://wiki.osdev.org/Raspberry_Pi_Bare_Bones) and [RaspberryPi.org manuals](https://www.raspberrypi.org/app/uploads/2012/02/BCM2835-ARM-Peripherals.pdf).

[![Built with cargo-make](https://sagiegurari.github.io/cargo-make/assets/badges/cargo-make.svg)](https://sagiegurari.github.io/cargo-make)
