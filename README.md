# Vesper

[![Join the chat at https://gitter.im/metta-systems/vesper](https://badges.gitter.im/metta-systems/vesper.svg)](https://gitter.im/metta-systems/vesper?utm_source=badge&utm_medium=badge&utm_campaign=pr-badge&utm_content=badge)

## About kernel

Vesper is a capability-based single-address-space exokernel, it tries to remain small and secure. To achieve this, kernel functionality is extremely limited - it provides only address space isolation and IPC, after bootup kernel does not allocate any memory itself.

Exokernel's distinctive trait is that it provides mechanisms but not policies. Vesper tries to move as many policy decisions as possible to the library OS.

* Single-address-space is a mechanism for providing pointer transparency between processes. Sharing a buffer is nearly as simple as passing out its address.

* IPC is a mechanism providing secure interaction between processes.

* Capabilities are a mechanism providing access rights control and universal authority delegation for OS objects.

* Interrupts come from hardware, usually in privileged mode and kernel is responsible for translating them into invocations of the device drivers' handlers.

### Scheduling

Scheduling can be viewed as the process of multiplexing the CPU resource between computational tasks. The schedulable entity of an operating system often places constraints both on the scheduling algorithms which may be employed and the functionality provided to the application. The recent gain in popularity of multi-threaded programming due to languages such as Modula-3 [Nelson 91] has led many operating system designers to provide kernel-level thread support mechanisms [Accetta 86, Rozier 90]. The kernel therefore schedules threads rather than processes. Whilst this reduces the functionality required in applications and usually results in more efficient processor context-switches, the necessary thread scheduling policy decisions must also be migrated into the kernel. As pointed out in [Barham 96], this is highly undesirable.

## Real Time

At the moment this is not a real-time kernel. It has a small number of potentially long-running kernel operations that are not preemptable (e.g., endpoint deletion and recycling, scheduling, frame and CNode initialisation). This may change in future versions.

## Credits
 
Vesper has been influenced by the kernels in L4 family, notably seL4. Fawn and Nemesis provided inspiration for single-address-space and vertical integration of the applications.

## Build instructions

