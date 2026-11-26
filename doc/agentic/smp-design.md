# SMP design

Status: nothing is implemented. This document holds the multicore rules (below) and their
mechanism; the capabilities contract links here for them. Open questions are in
[smp-decisions.md](smp-decisions.md), the work in
[smp-implementation-plan.md](smp-implementation-plan.md).

## Decisions

All by the maintainer, 2026-11-25.

- **Bring-up (C1(a)).** Kickstart starts the secondary cores at EL2, just before its own
  drop to EL1; they enter the nucleus at EL1 and idle until they have work. The nucleus
  has no EL2 code and no CPU-control kind.
- **Start-method components (C1).** The SMP operations have several implementations, two
  now (spin table for the Pi 3 and Pi 4, PSCI for the Pi 5), more later. Each is a
  position-independent component behind one operation table, like the interrupt
  controllers: Kickstart picks the one the device tree's `enable-method` names, once at
  boot, and installs only that one (see [start-method components](#start-method-components)).
- **Kickstart only (C1.1(a)).** Only Kickstart uses the component, to start the cores;
  its memory is reclaimed with Kickstart's. The nucleus gets a table when a later
  operation (core off, suspend, PSCI system off or reset) needs one.
- **Concurrency (C2(c)).** One kernel lock now: a ticket spinlock taken on every kernel
  entry, with per-core state outside it. No core waits for another while holding it.
  Finer locking is a later, measured pass.
- **Per-core record (C3(a)).** Kickstart carves one record and one trap stack per core
  from accounted memory, sized from the device tree's `/cpus`; the record's address is in
  `TPIDR_EL1`.
- **CPU numbering (C9(a)).** A CPU is named by its logical index `0..n-1` in the order of
  the `/cpus` nodes, whatever index the boot core has. This index is the `Dispatch` `x6`
  CPU, the Thread record's `cpu` and a Scheduler's core.
- **Schedulers and cores (C4(c)).** Each core has its own root Scheduler, root dispatcher
  and boot Time. A Thread runs only on the core of its Scheduler and moves to another
  core's Scheduler by a cross-core Thread move. `Scheduler.replace` and non-blocking
  dispatch stay parked in [scheduling-decisions.md](scheduling-decisions.md).
- **Scheduler core (C4.1(a)).** A Scheduler has a core field set once: Kickstart sets it
  for the root Schedulers, and a child Scheduler takes its parent's core at `Adopt`.
- **Bound Time on a move (C4.2(a)).** A cross-core move requires the Thread's bound Time
  to be empty and rebinds the Thread, in the same call, to a Time of the destination
  core. The old Time stays with the source Scheduler.
- **Move operation (C4.3(a)).** `Scheduler.MoveThread` carries the cross-core move: besides
  a Scheduler and its direct child, it moves a Thread between Schedulers of different
  cores, taking the destination Time key, with `WRITE` on both Schedulers, the Thread and
  the destination Time.
- **Placement (C5(b)).** A pinned component runs on its core; an unpinned one is placed by
  dispatcher policy at run time.
- **Pin declaration (C5.1(c)).** A component declares its core next to its priority (a
  byte in `driver_threads!` entries and an `entry_cpu!`-style section for the first
  Thread), and its manifest entry may override it. Kickstart hands the result to the
  dispatchers like `DispatchInit::priorities`.
- **No default pin (C5.2(b)).** Components are not pinned to the boot core by default,
  so whatever breaks on several cores shows up and is fixed.
- **Deadlines (C7(a)).** Each core has its own deadline queue: wait timeouts and
  dispatcher timers are armed on the core of the Scheduler that owns the waiting Thread
  or the dispatcher.
- **Device interrupts (C6: (a) now, (b) later).** Every device line goes to the boot
  core; a Notification or EventCount whose waiter is on another core sends that core an
  inter-processor interrupt. Routing a line to its receiver's core comes later.
- **Retiring what runs elsewhere (C8: (a) now, (b) later).** Retiring a Thread, or an
  AddressSpace current on other cores, stays one synchronous call: the retiring core
  marks the object dying, interrupts the cores using it, releases the kernel lock until
  each has switched away and acknowledged, then completes. Asynchronous retirement with
  quiescence detection comes later.
- **Bring-up order (C10(a)).** QEMU `raspi3b` and `raspi4b`, then the Pi 4, Pi 3 and Pi 5
  boards.
- **Record totals per core (C11(a), 2026-11-26).** Each core's root Scheduler record
  carries that core's own nucleus totals (`interrupt_ns`, `idle_ns`, `thread_switches`,
  `fp_thread_switches`, `fp_entry_toggles`), written only by that core. A whole-machine
  report is the client's own sum or average over the per-core reports; the nucleus keeps
  no machine-wide total.

## 1. What exists today

| Area | Today | Where |
|---|---|---|
| Entry | Every core that reaches `_boot_cores` (and chainboot's `_start`) compares its full affinity (Aff3, Aff2..Aff0) with `BOOT_CORE_AFFINITY` (0); the others park on `wfe` forever | `libs/boot/src/arch/aarch64/boot.s`, `boot.rs`; `bin/chainboot/src/boot.s` |
| Core id | `libcpu::smp::CoreAffinity` (the `MPIDR_EL1` affinity fields, in PSCI `CPU_ON` layout); no logical index yet | `libs/cpu/src/arch/aarch64/smp.rs` |
| Kernel state machine | `Init → SingleCoreMain → MultiCoreMain`; the last transition does not exist | `libs/kernel-state/src/lib.rs` |
| Kernel lock | `KERNEL_LOCK: IRQSafeNullLock<()>`, which protects nothing against another core; a TODO asks for the read-only / per-cpu / atomic / locked split | `kernel/nucleus/src/main.rs:58-62`, `libs/locking` |
| Per-core state | Kept as plain fields of the one `Nucleus`: `current_thread`, `current_since_ticks`, `dispatched`, `dispatch_next`, `preempt_requested`, `interrupt_ticks`, `idle_ticks`; plus statics `IDLING`, `timer::PROGRAMMED`, the FP owner in `fp_unit` | `kernel/nucleus/src/objects/nucleus.rs:89-132`, `main.rs`, `timer.rs`, `objects/arch/fp_unit.rs` |
| Kernel stack | One 512 KiB trap stack per core in `SP_EL1`, mapped by Kickstart in slots after the nucleus's `__STACK_VIRT_BOTTOM` with guard pages; a per-core record (`nucleus::core_record`) in `TPIDR_EL1`, set for the boot core only; the boot Thread runs EL1t on the init stack after the image | `kernel/kickstart/src/cores.rs`, `paging.rs`, `el_switch.rs` |
| EL2 → EL1 | Kickstart runs at EL2 with the MMU off, builds the tables, then `el_switch` sets VBAR, MAIR, TCR (inner shareable), SCTLR and drops to EL1t | `kernel/kickstart/src/el_switch.rs` |
| TLB maintenance | Nucleus already uses broadcast forms (`tlbi vae1is`, `tlbi aside1is`, `dsb ish`); Kickstart's boot flush is local `tlbi vmalle1` | `objects/arch/aarch64_objects.rs`, `el_switch.rs` |
| Interrupt controller ABI | `IrqChipOps`: `init`, `claim` (on the calling CPU), `complete`, `mask`, `unmask`, `set_trigger`, `xlate`. No per-CPU init, no IPI, no routing | `libs/irqchip/src/lib.rs` |
| BCM2836 component | Drives core 0's local registers only; core 1–3 timer, mailbox and source registers are reserved fields | `drivers/irqchip-bcm2836/src/lib.rs` |
| GIC-400 component | Routes every SPI to the CPU interface of the core running `init` (from banked `GICD_ITARGETSR0-7`) | `drivers/irqchip-gicv2/src/lib.rs` |
| Scheduling ABI | `Scheduler.Dispatch` `x6` is the CPU and must be 0; the Thread record has a `cpu` field (`0` or `NO_CPU`); a Time "belongs to one core" and Transfer/Merge already carry per-core checks | [scheduling design](scheduling-design.md#decisions); `objects/time.rs` |
| Payload ordering | The contract states that multicore needs real Acquire/Release kernel atomics for Signal/Advance publication | [contract, Notification and EventCount](capabilities-contract.md#notification-and-eventcount) |
| Reserved memory | Kickstart removes `/memreserve/` entries and the `no-map` nodes of `/reserved-memory` from the Untypeds | `kernel/kickstart/src/lib.rs`, `libdevicetree::DeviceTree::no_map_reserved_memory` |
| QEMU | `raspi3b` and `raspi4b` always model four cores | `Justfile` |

## 2. How the secondary cores are started

The device trees in `targets/` name the method per CPU node:

| Board | Cores | `enable-method` | Release | Entry state |
|---|---|---|---|---|
| Pi 3 (BCM2837, Cortex-A53) | 4, MPIDR Aff0 = 0..3 | `spin-table`, `cpu-release-addr` 0xd8, 0xe0, 0xe8, 0xf0 | Write the entry's physical address to the core's 64-bit word, clean it to PoC, `dsb sy`, `sev` | The firmware stub (`armstub8`) left the core at EL2, MMU and caches off, `x0` = 0 |
| Pi 4 (BCM2711, Cortex-A72) | 4, Aff0 = 0..3 | `spin-table`, same addresses | Same | Same |
| Pi 5 (BCM2712, Cortex-A76) | 4, **Aff1** = 0..3 (`reg` 0x000, 0x100, 0x200, 0x300) | `psci`, `method = "smc"`, PSCI 1.0 | `SMC` with `x0 = 0xC400_0003` (`CPU_ON`, SMC64), `x1` target MPIDR, `x2` entry physical address, `x3` context id; `x0` = 0 on success | Entered by TF-A (bl31) with the MMU off and the context id in `x0`; the entry EL (expected EL2, as the boot core) is to be checked against DEN0022 and on the board |
| QEMU `raspi3b`/`raspi4b` | 4 | spin table at the same addresses, written by QEMU's own boot stub (`hw/arm/raspi.c`, `write_smpboot64`) | Same as Pi 3 | QEMU starts the cores at EL3 (the boot core's `qemu` path already drops EL3 → EL2) |

Consequences:

- **Aff0 alone is wrong on the Pi 5**: every core reads Aff0 = 0, so a core is named by
  its full affinity (`CoreAffinity`). The affinity is not the CPU number either: the
  kernel needs a logical CPU index, assigned from the order of the `/cpus` nodes and kept
  in the per-core record.
- The start method is a component Kickstart selects from `enable-method` ([start-method
  components](#start-method-components)).
- The firmware's spin code and words live in the first page (`/memreserve/ 0 0x1000` on
  the Pi 3 and 4). On the Pi 5, TF-A occupies `0..0x80000` (`/reserved-memory/atf@0`,
  `no-map`). Kickstart reserves both kinds, each page once
  (`BootInfo::insert_reserved_region`), and nothing of Vesper's lives below the image
  base `0x80000`: Kickstart's init stack follows its image (`linker/kickstart.ld`) and
  chainboot's stack its relocated BSS above 32 MiB (`bin/chainboot/src/link.ld`).
- A secondary core starts with its own EL2 registers unset. `_startup_in_rust`'s EL2
  setup (`CNTHCTL_EL2`, `CNTVOFF_EL2`, `SCTLR_EL2`, `HCR_EL2`, `CPTR_EL2`) and, under
  `qemu`, the EL3 → EL2 step, must run on every core, followed by `el_switch` with the
  same `TTBR1_EL1`, `TCR_EL1`, `MAIR_EL1`, `VBAR_EL1` and the core's own `SP_EL1`.
- Cache coherency: Cortex-A53/A72 need `CPUECTLR_EL1.SMPEN` set before caches and MMU
  are enabled. The Pi firmware stub sets it at EL3 on every core and TF-A does so on the
  Pi 5; Vesper cannot write it from EL2 (`ACTLR_EL3` gates it). Verify on the board; QEMU
  does not model coherency.

### Mixed-attribute handshake

The releasing core runs with caches on (if the nucleus releases) or off (if Kickstart
releases from EL2, where its MMU is off). A secondary runs with the MMU off until its
`el_switch`, so its accesses are Non-cacheable and atomic read-modify-write does not
complete on the Pi's cores (the chainboot rule). The rule that follows:

- The releasing side writes the spin word and the per-core boot parameters with plain
  stores and cleans them to PoC before `sev`.
- The secondary reads its parameters with plain loads, enables its MMU and caches, and
  only then signals "online" through a cacheable atomic, which the boot core reads
  coherently.

### Start-method components

Modelled on the interrupt-controller components (`libs/irqchip`, `drivers/irqchip-*`,
`kernel/kickstart/src/privileged.rs`):

- An ABI crate (`libs/cpu-start`) defines the operation table every
  implementation exports, its version, and the configuration Kickstart passes. A first
  table: `abi_version`, `init(config)`, `start(cpu, mpidr, entry, context) -> status`.
  `entry` is the physical address of the secondary entry, `context` the physical address
  of that core's boot parameters.
- Implementations, each a position-independent blob in the boot image with an
  `enable-method` compatible list in its own section:
  - `cpu-start-spin-table` (`"spin-table"`): config holds each core's
    `cpu-release-addr`; `start` writes the entry, cleans the word to PoC, `dsb sy`, `sev`.
    The spin table passes no argument, so the secondary entry finds its parameters by
    its MPIDR in a table Kickstart wrote.
  - `cpu-start-psci` (`"psci"`): config holds the conduit from the `psci` node (`smc` or
    `hvc`); `start` issues `CPU_ON` with `context` in `x3`, which arrives in the
    secondary's `x0`, and maps PSCI errors to the status.
- Refusing means running on the boot core alone (maintainer, 2026-11-26): a tree whose
  `/cpus` nodes disagree, a method no bundled component implements, more CPUs than the
  ABI describes or a configuration `init` refuses starts no secondary core, and Kickstart
  logs why.
- Kickstart reads the `enable-method` of the `/cpus` nodes once, refuses a tree whose
  nodes disagree, relocates the matching component and calls it at EL2 (its MMU is off,
  so the component runs at its physical load address). The other implementations are
  not loaded.
- The nucleus does not get the table; a later operation that needs one (core off,
  suspend) adds the hand-over then.

## 3. What the nucleus needs

### Per-core record

One record per core, carved by Kickstart from accounted memory (the kernel never
allocates), its address in `TPIDR_EL1` (EL1-only, so no user exposure):

- logical CPU index and MPIDR;
- the trap stack top (each core needs its own `SP_EL1` stack; the contract already says
  "one stack per core");
- `current_thread`, `current_since_ticks`, `dispatched`, `dispatch_next`,
  `preempt_requested`, idle state (`IDLING`), interrupt and idle tick totals;
- the programmed one-shot deadline (`timer::PROGRAMMED`), since `CNTP_*_EL0` is per core;
- the FP owner (`fp_unit`), since the FP register file is per core;
- the pending IPI bits.

Everything else stays in the shared `Nucleus` (pools, key tables, pending records, IRQ
table, activations). Each core has its own deadline queue (wait timeouts and dispatcher
timers of its Schedulers) in its record. The existing scheduler-record
totals (`interrupt_ns`, `idle_ns` and the FP totals) become per-core fields, published in
each core's root Scheduler record (C11(a)).

### Kernel lock

The `Access` design (owning context, short-lived guards, no overlapping `&mut`) already
assumes one owner at a time. The smallest change that keeps it sound is a real lock in
place of `IRQSafeNullLock`: a ticket spinlock (`ldaxr`/`stlxr`; the Cortex-A53 and A72
have no LSE) taken on every kernel entry (SVC, IRQ, fault) after the per-core record is
found, and released on exit. Rules:

- **Never wait for another core while holding it.** A core spinning on the lock has
  interrupts masked and cannot take an IPI.
- **The idle loop releases it around `wfi`**, and the interrupt taken there re-takes it
  before claiming. Today the idle loop runs inside the trap with the kernel state held.
- Per-core record fields are touched only by their own core, without the lock, or under
  the lock by another core only where noted (pending IPI bits are atomics).
- Kickstart keeps `IRQSafeNullLock`: it runs on one core with the MMU off, where an
  exclusive never completes.

Finer locking is a later, measured pass.

### Inter-processor interrupts

The contract already names the IPI as the way a signal reaches a waiter on another
core. The kinds needed:

| IPI | Sent when | Receiver does |
|---|---|---|
| Reschedule | A Thread or dispatcher bound to that core became runnable or was activated; a remote `replace` | Re-enters the kernel exit path, which selects |
| Stop | A Thread running there is retired, faulted by a peer, or replaced; an AddressSpace active there is retired | Parks or drops the Thread, switches `TTBR0` away, acknowledges |
| Halt | Panic | Masks everything and parks |

Mechanism per controller:

- **BCM2836 local controller (Pi 3):** per-core mailboxes. Write-set at
  `0x4000_0080 + 0x10·core + 4·mailbox`, read/write-clear at `0x4000_00C0 + ...`, enable
  in `CORE_MAILBOX_INT_CONTROL` (`0x4000_0050 + 4·core`). Local lines 4..7 are already
  numbered `LINE_MAILBOX0..3` in the component.
- **GIC-400 (Pi 4, Pi 5):** SGIs through `GICD_SGIR` (target list bits 23:16, INTID
  bits 3:0). SGI/PPI enables, priorities and the CPU interface (`GICC_CTLR`, `GICC_PMR`)
  are banked, so each core initialises its own.

The IPI kind travels in the per-core record (atomic bits); one hardware line or SGI is
enough.

### Interrupt-controller ABI

`IrqChipOps` grows, in place (no backwards compatibility):

- `init_cpu(cpu)`: called on each core at bring-up. BCM2836: enable that core's timer
  (`CORE_TIMER_INT_CONTROL` CNTPNS bit) and mailbox 0 IRQ. GIC: enable the CPU interface,
  priority mask and the timer PPI on this core.
- `send_ipi(cpu_mask)`.
- `claim` already works "on the calling CPU"; the component's data (`static` in its
  `main.rs`) is shared, which the kernel lock covers.
- Per-line routing (`route(line, cpu)`) comes later; at first every device line goes to
  the boot core, as the GIC component already does.

### Timers

`CNTP` is per core and the counter is common, so the clock contract is unchanged. Each
core programs its own one-shot for its running Thread's dispatch limit and budget. Wait
timeouts and `Scheduler.ArmTimer` deadlines go in the per-core queue of the core that
owns the Scheduler, and that core's one-shot is programmed to the earliest of its limit,
budget and queue head.

### Memory management

- Translation updates are already break-before-make with `dsb ishst` and broadcast TLBI;
  nothing to add for remote TLBs on Arm.
- Any set/way cache maintenance is local only (`libs/cpu` cache notes); by-VA operations
  to PoC are broadcast. Audit the nucleus and drivers for set/way use.
- `TTBR0_EL1` is per core. An AddressSpace may be current on several cores at once
  (several of its Threads). Retiring it, or recycling its ASID, needs every core to have
  switched away first, which is the Stop IPI and an "active on" core mask in the
  AddressSpace. Retirement waits for every core's acknowledgement with the kernel lock
  released, then completes in the same call; asynchronous retirement with quiescence
  detection is later work.

### FP/SIMD

The save is eager at the switch away (FP/SIMD design, "Multicore, later"), so no FP
state is left in another core's registers: the owner variable becomes per core and
nothing else changes.

### Panic, console and logging

The panic handler stops the other cores with the Halt IPI. Semihosting and the board
UART print from several cores, so a line must be written under a lock (the kernel lock in
the kernel; `libs/log`'s `static mut LOGGER` is set once at boot and is fine).

## 4. Scheduling

The selected model already carries a CPU: dispatch names one, a Time belongs to one
core, the record has a `cpu` field. Each core gets its own root Scheduler and root
dispatcher, funded by that core's boot Time; a Thread runs only on the core of its
Scheduler. Load balancing is dispatcher policy that moves an unpinned Thread to another
core's Scheduler with the cross-core move, which needs the Thread's bound Time empty and
rebinds it to a Time of the destination core, through `Scheduler.MoveThread`. A
centralized dispatcher
placing Threads on remote cores would need `replace` and non-blocking dispatch, which
stay parked.

## 5. Userspace

Today every component runs on one core, so any shared-memory protocol between Threads
was only ever exercised with one Thread running at a time. Before a component's Threads
run on different cores at once, its shared data must use real atomics with the right
ordering (IO channels, EventCount-published rings, `log-server`, the audio ring, the
compositor's surfaces). A pinned component stays on its core; an unpinned one goes where
the dispatchers place it. Nothing is pinned by default, so such ordering bugs surface on
the boards and are fixed where they show up.

## 6. Validation

- QEMU `raspi3b` (spin table, BCM2836 mailboxes) and `raspi4b` (spin table, GIC SGIs)
  cover bring-up and IPIs. Neither models caches, so coherency and `SMPEN` need boards.
- The Pi 5 (PSCI, Aff1 numbering) is testable only on the board.
- Board test kernels keep printing every ≤20 s and park the boot Thread on a
  Notification.
- An e2e test kernel per stage, in the style of `test-preempt`/`test-irq`, with in-guest
  assertions and the QEMU exit status as the result.

## Sources

Section numbers are to be filled in when each stage cites them; none was checked during
this investigation.

- Arm Architecture Reference Manual for A-profile: `MPIDR_EL1`, `TPIDR_EL1`, broadcast
  TLB maintenance, mismatched memory attributes.
- Arm PSCI specification DEN0022: `CPU_ON` and the entry state of a started core.
- ARM GIC architecture specification v2 (IHI 0048B): `GICD_SGIR`, banked registers.
- Cortex-A53 and Cortex-A72 TRMs: `CPUECTLR_EL1.SMPEN`.
- BCM2836 ARM-local peripherals (QA7): core mailboxes, core timer interrupt routing.
- Linux `Documentation/arch/arm64/booting.rst`, spin-table protocol.
- raspberrypi/tools `armstubs/armstub8.S`; QEMU `hw/arm/raspi.c`.
- Device trees in `targets/` (`bcm2710-rpi-3-b-plus.dtb`, `bcm2711-rpi-4-b.dtb`,
  `bcm2712-rpi-5-b.dtb`).
