# Interrupt controller placement: prior art

Where should the interrupt-controller driver run? On IRQ entry the kernel must acknowledge or
mask the line before returning to user mode, or a level-triggered line fires again immediately.
The preemption tick (ARM generic timer PPI) arrives through the same controller.

## Options

- **A: the kernel owns the controller.** The kernel does ack/mask. Routing, trigger mode and
  affinity are fixed by the kernel or boot configuration. Userspace may still hold per-line
  handles and choose which line goes to which driver.
- **B: a userspace controller.** The kernel only masks at the CPU (DAIF) and forwards every
  IRQ to an IC component, which demultiplexes and acks.
- **C: hybrid.** The kernel does ack/mask (mechanism). A capability holder in userspace also
  configures the lines themselves (trigger, polarity, target CPU), which is policy.

A and C differ in degree: most capability kernels let userspace decide line→driver
assignment; "C" here means userspace can also reconfigure the line itself.

## Observations

- **No pure B.** No surveyed system puts the root controller in userspace. Barrelfish is the
  only B example, and only for a secondary controller (x86 I/O APIC) behind a kernel-owned
  LAPIC.
- **Common delivery protocol.** The kernel masks (or EOIs without deactivating) on delivery,
  signals a kernel object, and the driver's ack unmasks or deactivates. seL4, NOVA, Coyotos,
  Hubris, QNX, L4, Genode and Zircon all work this way.
- **GIC idiom.** GIC split priority-drop/deactivate (EOImode) is how NOVA, and seL4 on GICv3,
  defer "done" to the driver without masking the line.
- **The timer always stays in the kernel.** Every surveyed system takes the preemption-timer
  interrupt in the kernel. Where scheduling policy is in userspace (Composite user-level
  schedulers, Nemesis domain activations), the kernel still enforces budgets and notifies the
  user scheduler. That matters if the clock becomes a userspace component.
- **Prior art for kickstart matching.** Linux `IRQCHIP_DECLARE` + `of_irq_init` and QNX startup
  callouts both select IC code from the board description before the kernel runs. That is the
  model for kickstart choosing the IC backend.

## Survey

Timer: who takes the preemption-timer interrupt.
Sources: `src` means checked in source code, `doc` in official documentation,
`paper` in a thesis or paper, `secondary` in secondary material only.

### seL4

| | |
|---|---|
| Placement | C (A-leaning) |
| Timer | kernel |
| Sources | `src` in seL4 `src/object/interrupt.c`, `src/arch/arm/object/interrupt.c`, `src/plat/bcm2837/config.cmake` (github.com/seL4/seL4, master). |

IC driver compiled into the kernel (RPi3: `drivers/irq/bcm2836-armctrl-ic.h`, timer `drivers/timer/arm_generic.h`). On IRQ the kernel signals the Notification bound to the IRQHandler, masks the line (not on GICv3) and acks. The driver's `IRQHandler_Ack` unmasks (GICv3: deactivates). IRQControl issues handlers via `IssueIRQHandler`; on ARM also `IssueIRQHandlerTrigger` (level/edge, only where the platform supports it), `IssueIRQHandlerTriggerCore` (SMP target core) and `IssueSGISignal`.

### Composite

| | |
|---|---|
| Placement | A (userspace assigns lines) |
| Timer | kernel enforces, user schedules |
| Sources | `src` in Composite `src/kernel/include/hw.h`, `src/kernel/capinv.c` (`cap_hw_asnd`, `timer_process`, `CAPTBL_OP_HW_*`), `src/platform/i386/lapic.c` (github.com/gwsystems/composite, main). |

A `CAP_HW` capability carries a bitmap of external IRQs (32–63). `HW_ATTACH`/`HW_DETACH` bind an IRQ to an asynchronous receive endpoint whose thread handles it. There is no trigger or routing op. On IRQ, `cap_hw_asnd` → `asnd_process` decides by temporal capabilities (tcaps) whether the interrupt thread preempts. The kernel's `timer_process` enforces tcap budgets and notifies the user-level scheduler thread on expiry.

### Nemesis

| | |
|---|---|
| Placement | C, plus driver code in kernel mode |
| Timer | kernel scheduler |
| Sources | `paper` in P. Barham, *Devices in a Multi-Service Operating System*, UCAM-CL-TR-403 (1996), §3.3.2 and Table 3.1 (NTSC calls), §3.7.1 (hardware interrupts). |

"The kernel demultiplexes hardware interrupts to the stage where a device specific first-level interrupt handler may be invoked." Privileged domains register stubs (`ntsc_regstub`) that run with all interrupts disabled. Stubs send events (`ntsc_kevent`) and return via `ntsc_rti`. Privileged domains can also change IPL (`ntsc_swpipl`) and enter kernel mode (`ntsc_entkern`).

### Barrelfish

| | |
|---|---|
| Placement | B for the secondary controller, C overall |
| Timer | CPU driver |
| Sources | `src, report` in Barrelfish source tree (github.com/BarrelfishOS/barrelfish): `kernel/arch/arm/gic*.c`, `usr/acpi/arch/x86/ioapic.c`, `lib/int_route`, `usr/kaluga/start_int_ctrl.c`; R. Afifi, *HPET Driver Report* (2018). |

The CPU driver owns the GIC (`kernel/arch/arm/gic_v2.c`, `gic_v3.c`) and the x86 LAPIC. The x86 I/O APIC driver is userspace (`usr/acpi/arch/x86/ioapic.c`). Kaluga starts the interrupt controllers, and an `int_route` service computes routing as a constraint problem in the system knowledge base (SKB).

### Coyotos

| | |
|---|---|
| Placement | A |
| Timer | kernel (`HardClock.c`) |
| Sources | `src` in Coyotos `src/sys/idl/IrqCtl.idl`, `IrqWait.idl`, `src/sys/caps/cap_IrqWait.c`, `src/sys/arch/i386/kernel/IRQ.c`, `src/sys/kernel/kern_IRQ.c` (github.com/vsrinivas/coyotos). |

On IRQ the kernel masks the line at the PIC/IOAPIC, acks, and wakes waiters. `IrqCtl` offers `getIrqWait`, `bindIrq`, `enable`, `disable` and `wait`. `IrqWait.wait` unmasks the line and blocks. There is no trigger or routing configuration.

### EROS

| | |
|---|---|
| Placement | A (drivers in kernel) |
| Timer | kernel |
| Sources | `paper` in J. Shapiro et al., *EROS: a fast capability system*, SOSP'99. |

"Bottom-half device drivers and the single-level store are therefore implemented within the kernel."

### Fiasco.OC / L4Re

| | |
|---|---|
| Placement | C (purest) |
| Timer | kernel |
| Sources | `doc` in L4Re documentation: `L4::Icu`, `L4vbus::Icu` (l4re.org/doc). |

The kernel exposes the IC as `L4::Icu` objects with `bind`, `unbind`, `set_mode`, `mask`, `unmask`, `info` and `msi_info`. The userspace Io server gives each client a virtual ICU on its vbus, so line policy lives in a userspace server.

### Fluke

| | |
|---|---|
| Placement | A |
| Timer | kernel |
| Sources | `paper, abstract and excerpts only` in K. Van Maren, *The Fluke Device Driver Framework*, MS thesis, Univ. of Utah (1999); B. Ford et al., *The Flux OSKit*, SOSP'97. |

Drivers are unmodified OSKit drivers run as user-mode servers. The kernel interrupt handler only signals a user-level driver thread.

### Genode (base-hw)

| | |
|---|---|
| Placement | C |
| Timer | kernel |
| Sources | `doc, mailing list` in Genode `repos/base/include/irq_session/irq_session.h`; Genode users list on base-hw IRQ delivery. |

Core hands out IRQ sessions with `Trigger` (level/edge) and `Polarity`. The kernel delivers interrupts straight to the client's signal context, and `ack_irq()` re-enables the line.

### Hubris

| | |
|---|---|
| Placement | A, build-time |
| Timer | kernel (SysTick) |
| Sources | `doc` in Hubris reference manual (hubris.oxide.computer/reference): interrupts, `IRQ_CONTROL`, SysTick. |

`app.toml` maps each IRQ to one task's notification bits, compiled into a kernel table. The kernel ISR posts the notification and disables the IRQ. The task re-enables it with `IRQ_CONTROL` (optionally clearing pending). "Drivers live in tasks, not the kernel."

### K42

| | |
|---|---|
| Placement | A (drivers in kernel) |
| Timer | kernel |
| Sources | `src` in K42 `os/kernel/linux/Interrupt.C`, `os/kernel/linux/arch/powerpc/LinuxPIC.C` (github.com/jimix/k42, kitchsrc). |

Linux device drivers and their IRQ handlers run inside the kernel through the `os/kernel/linux` glue (`Interrupt.C`, `LinuxPIC.C`).

### KeyKOS

| | |
|---|---|
| Placement | — |
| Timer | — |
| Sources | `secondary` in MIT 6.828 microkernel lecture notes (2010), KeyKOS section. |

Devices are kernel objects handed to driver processes as keys. No source found for the interrupt path (S/370 channel I/O).

### L4 X.2 / Pistachio (and Mungi on it)

| | |
|---|---|
| Placement | A |
| Timer | kernel |
| Sources | `doc, mailing lists` in L4 eXperimental Kernel Reference Manual X.2 (L4Ka, rev. 2009); l4-hackers list threads on interrupt-thread conventions (2002, 2011). |

Interrupts appear as IPC from interrupt "threads" (special thread IDs). The privileged root task associates a handler with `ThreadControl`. The kernel masks and acks; the handler re-enables the line by replying with an IPC. The kernel keeps the timer interrupt.

### Linux

| | |
|---|---|
| Placement | A |
| Timer | kernel |
| Sources | `src` in Linux `include/linux/irqchip.h` (`IRQCHIP_DECLARE`), `drivers/of/irq.c`. |

irqchip drivers in `drivers/irqchip` bind a DT `compatible` to an init function with `IRQCHIP_DECLARE`, run by `of_irq_init`. Userspace only tunes affinity (`/proc/irq/N/smp_affinity`).

### macOS (XNU)

| | |
|---|---|
| Placement | A |
| Timer | kernel |
| Sources | `src, doc` in XNU `iokit/IOKit/IOInterruptController.h`; Apple DriverKit `IOInterruptDispatchSource`; Linux `drivers/irqchip/irq-apple-aic.c`. |

Interrupt controllers are IOKit `IOInterruptController` subclasses in the kernel (`registerInterrupt`, `enableInterrupt`, `disableInterrupt`). The Apple Silicon AIC driver is closed source, though its hardware is documented by Asahi's Linux driver. DriverKit userspace drivers receive interrupts through `IOInterruptDispatchSource`.

### MINIX 3

| | |
|---|---|
| Placement | A |
| Timer | kernel |
| Sources | `src` in MINIX 3 `minix/kernel/system/do_irqctl.c`. |

The kernel owns the PIC/APIC. `sys_irqctl` supports `IRQ_SETPOLICY`, `IRQ_RMPOLICY`, `IRQ_ENABLE` and `IRQ_DISABLE`, with lines restricted by the process privilege table.

### NOVA

| | |
|---|---|
| Placement | C |
| Timer | kernel |
| Sources | `src` in NOVA `src/aarch64/interrupt.cpp`, `src/syscall.cpp` (`sys_assign_int`, `sys_ctrl_sm`) (github.com/udosteinberg/NOVA, release). |

Kernel owns GIC/LAPIC/IOAPIC. aarch64: on entry it reads IAR and EOIs (priority drop), then does `Sm::up()` on the semaphore attached to the SPI. Deactivation (`GICC_DIR`) is deferred until the driver's `ctrl_sm` down on that semaphore. `assign_int` (needs an SM cap with ASSIGN) attaches the semaphore and sets target CPU and config bits (mask, level/edge, guest-owned). The EL2 physical timer PPI is consumed by the kernel (`Timeout::check`).

### Opal

| | |
|---|---|
| Placement | inherits Mach 3.0 (A) |
| Timer | kernel |
| Sources | `secondary` |

Ran on Mach 3.0, whose device drivers are in the kernel.

### QNX

| | |
|---|---|
| Placement | A |
| Timer | kernel (timer callouts) |
| Sources | `doc` in QNX *Building Embedded Systems*: "interrupt_id_*() and interrupt_eoi_*()", "Interrupt controller" and "Timer and clock" callouts (7.1/8.0). |

The board support package's startup code supplies interrupt callouts: `interrupt_id_*`/`interrupt_eoi_*` (copied into the kernel, which masks the asserted line) and `mask`/`unmask`/`config`. Startup code sets trigger mode (only level-sensitive is used). The kernel drives the system timer through the `timer_load`/`timer_reload` callouts. Drivers are userspace processes using `InterruptAttach*`.

### Windows

| | |
|---|---|
| Placement | A |
| Timer | kernel |
| Sources | `doc` in Microsoft Learn: "Servicing an Interrupt (UMDF)", `WUDF_INTERRUPT_ISR`, "Supporting Passive-Level Interrupts". |

The HAL owns the controller, and kernel drivers' ISRs and DPCs run in the kernel. UMDF 2 drivers get a passive-level ISR. For level-triggered lines the OS interrupt dispatch thread blocks in the kernel waiting for the user-mode driver's answer, so only MSI or exclusive edge interrupts are recommended.

### Zircon

| | |
|---|---|
| Placement | C |
| Timer | kernel |
| Sources | `doc` in Fuchsia `zx_interrupt_create` reference (fuchsia.dev). |

`zx_interrupt_create` needs an IRQ resource and takes the trigger mode and polarity (`ZX_INTERRUPT_MODE_{EDGE,LEVEL}_{LOW,HIGH}`). `zx_interrupt_ack` requires WRITE rights.

## Event delivery and temporal capabilities (Composite, seL4)

Evidence for the [IRQ delivery](irq-design.md#irq-delivery) contract and the [Time](scheduling-design.md#decisions) rules. Composite read from its repository history; seL4 from a 2026-01 tree (`04db8cebc`).

### Composite: brands, acaps, then asynchronous endpoints with tcaps (source-verified)

- **Brands, 2007-10-02 (`04a234c356` "Primitive branding support") to 2014-01-14 (`53c8e578cf` "Removed brand mechanism completely").** A brand is a `struct thread` flagged `THD_STATE_BRAND`: an event principal holding a list of upcall threads (`THD_STATE_UPCALL`) and a pending-upcall count. `COS_BRAND_CREATE` copies the creator's invocation stack, cut at a destination component, and its scheduling information. `cos_syscall_brand_wire` wires a brand only to the timer or a network port. `brand_next_thread` switches to the upcall thread when the brand outranks the running thread, otherwise it counts the event as pending.
- **Acaps, 2013-06-18 to 2014-02-14.** Built "based on brand_ implementation": a client acap in the sending component paired with a server acap bound to exactly one upcall thread, an optional owner thread and a core. `ainv_send`/`ainv_wait`; cross-core sends become IPIs. Wiring still covers only timer and network.
- **Current `main`.** `CAP_ARCV` is bound to one thread with a tcap and a parent scheduler (`notify_parent`); `CAP_ASND` names an arcv and is the distributable send right; `asnd_process` compares tcaps (`tcap_higher_prio`) to switch at once or leave the event pending. `CAP_HW` holds an IRQ bitmap and `HW_ATTACH(irq, arcv)` creates a kernel send endpoint per line, so an interrupt is a kernel send. There are no trigger, mask or acknowledge operations.
- **Tcaps (`kernel/tcap.c`, `include/tcap.h`, `kernel/capinv.c`).** `TCAP_ACTIVATE` makes a tcap from kernel memory with zero cycles. `TCAP_TRANSFER` (to an arcv's tcap) and `TCAP_DELEGATE` (through an asnd, optionally switching) move cycles permanently and merge the `delegations[16]` chain of `(tcap_uid, prio)`; `TCAP_MERGE` moves everything then deletes the source (refused while referenced). A tcap belongs to one core. The user-level switch names (thread, tcap, priority, timeout); `cap_update` charges the current tcap and `tcap_timer_update` programs a one-shot timer at the earlier of budget exhaustion and the timeout. On exhaustion `expended_process` switches to the tcap's scheduler thread and notifies it; a scheduling token (`cap_sched_tok_validate`) rejects stale user-level dispatch decisions. Exhaustion keeps only the current scheduler's priority ("declassifies" the chain). There is no parent link or loan.

### seL4: IRQControl, IRQHandler and Notification binding (source-verified)

- `irq_control_cap`/`irq_handler_cap` are generic cap types (`include/object/structures_64.bf`) with generic operations in `src/object/interrupt.c`: `IssueIRQHandler`, `AckIRQ`, `SetIRQHandler`, `ClearIRQHandler`. Architecture code adds only issue variants (`src/arch/arm/object/interrupt.c`: trigger, target core, `SGISignal`); the controller is compile-time `getActiveIRQ`/`maskInterrupt`/`ackInterrupt`.
- `SetIRQHandler` stores a copy of a Notification cap (send right required) in the kernel slot `intStateIRQNode[irq]`; the badge comes from that copy. `handleInterrupt` signals it, masks the line (except GICv3) and EOIs; `AckIRQ` unmasks (GICv3: deactivates; remote PPIs through an IPI). Issuing an active line fails with `RevokeFirst`.
- Who runs: any thread waiting on the Notification, or the thread bound with `TCB_BindNotification` (one-to-one; it also wakes a thread blocked in endpoint receive, `src/object/notification.c`). The woken thread's priority decides through `possibleSwitchTo`. With MCS, `SchedContext_Bind` on a Notification lends a scheduling context to a passive handler on signal.

### Consequences for Vesper

- Every role a brand had has an owner: pending events (Notification), handler (receiver Thread), scheduling identity (the receiver's Time), cross-core delivery (kernel IPI). A thread-plus-brand object would recreate Composite's 2007 design, which Composite removed.
- seL4 keeps IRQ capabilities generic although its controllers are compiled in; Vesper's controller is already behind a runtime component table, so the kinds are core.
- Per-thread budgets would force a parent scheduler to split budget into every thread of every child scheduler; per-dispatch binding with receiver-owned budgets (Composite) keeps the hierarchy encapsulated.
