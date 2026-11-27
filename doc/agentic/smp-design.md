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
- **Scheduler queues (maintainer, 2026-11-27).** The root scheduler's run queues are its
  own: it carves the queues it carved for one core once per core, owns them, and the
  kernel knows nothing of them.
- **Kernel run state per core (maintainer, 2026-11-27, (c)).** The kernel's queue of
  dispatchers made runnable ahead of what runs (`activations`) is split per core: fixed
  per-core lists in the `Nucleus`, a Scheduler's dispatcher on its core's list, which
  another core may push onto under the kernel lock before it sends `IPI_RESCHEDULE`. The
  table of armed `ArmTimer` deadlines stays one table, each entry tagged with its
  Scheduler's core; each core programs its own timer from its own entries.
- **Cross-core activation (maintainer, 2026-11-27, (b)).** Cores stay loosely coupled, so
  that the kernel lock can later be split or removed: a core never writes another core's
  `CoreState`. An activation of another core's dispatcher completes its parked `Dispatch`
  (shared object state), queues the dispatcher on that core's list and sends it
  `IPI_RESCHEDULE`; the target core brings its own `dispatched` up to date when it takes
  the IPI.
- **Root dispatcher layout (maintainer, 2026-11-27).** One root dispatcher component, one
  AddressSpace, with one Thread per core. Each Thread works its own core's area of the
  scheduler's memory; operations across cores (moving a Thread to another core, pinning
  it) are done cooperatively inside that AddressSpace.
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
  inter-processor interrupt. Routing a line to its receiver's core comes later. The
  device lines reach the boot core today: the GIC component targets every SPI at the CPU
  interface of the core running its `init` (the boot core, at the platform install), the
  BCM2836 component routes the cascaded block to that core, and a secondary unmasks only
  its IPI line. The IPI to a waiter on another core is built with the Scheduler's core
  field, in Phase 11 of
  [scheduling-implementation-plan.md](scheduling-implementation-plan.md) (maintainer,
  2026-11-26): before it, every waiter is on the boot core.
- **Retiring what runs elsewhere (C8: (a) now, (b) later).** Retiring a Thread, or an
  AddressSpace current on other cores, stays one synchronous call: the retiring core
  marks the object dying, interrupts the cores using it, releases the kernel lock until
  each has switched away and acknowledged, then completes. Asynchronous retirement with
  quiescence detection comes later. It is built in Phase 11 of
  [scheduling-implementation-plan.md](scheduling-implementation-plan.md), once Threads
  run on other cores (maintainer, 2026-11-26): before that nothing runs, and no
  AddressSpace is current, anywhere but the boot core.
- **An AddressSpace in use elsewhere (C8.1(a), 2026-11-27).** `AddressSpace.Retire`
  fails with `InvalidOperation` while one of its Threads runs on another core: the
  maintainer's words, "it's up to the userspace policy to schedule retirement at the
  correct time". A core that only still holds it in `TTBR0_EL1` (idle after one of its
  Threads) gets a Stop, turns TTBR0 walks off and lets go of it; the call is retried
  until no core holds it, then releases the ASID.
- **Page tables named elsewhere (C8.2(a), 2026-11-27).** `PageTable.Unmap` of a root
  does the same check and Stop, so after it no core's `TTBR0_EL1` names the root.
- **Bring-up order (C10(a)).** QEMU `raspi3b` and `raspi4b`, then the Pi 4, Pi 3 and Pi 5
  boards.
- **Record totals per core (C11(a), 2026-11-26).** Each core's root Scheduler record
  carries that core's own nucleus totals (`interrupt_ns`, `idle_ns`, `thread_switches`,
  `fp_thread_switches`, `fp_entry_toggles`), written only by that core. A whole-machine
  report is the client's own sum or average over the per-core reports; the nucleus keeps
  no machine-wide total.
- **Secondary EL1 stack (C12(a), 2026-11-26).** A secondary core enters EL1h on its own
  trap stack (`SP_EL1`) with no execution stack: `el_switch` takes the EL1 mode from its
  caller (EL1t on `SP_EL0` for the boot core, EL1h for a secondary). Until it enters its first
  Thread its code runs as the nucleus's idle path does, and no other stack is carved for it.
- **No multicore kernel state (C13(a), 2026-11-26).** SMP keeps no kernel lifecycle
  state: what multicore depends on is the kernel lock (SMP2). `libkernel-state` is only
  the init-phase flag `InitStateLock::write` checks (maintainer, 2026-11-26: the
  `SingleCoreMain`/`MultiCoreMain` state machine, which nothing moved, is removed).

## 1. What exists today

| Area | Today | Where |
|---|---|---|
| Entry | Every core that reaches `_boot_cores` (and chainboot's `_start`) compares its full affinity (Aff3, Aff2..Aff0) with `BOOT_CORE_AFFINITY` (0); the others park on `wfe` forever | `libs/boot/src/arch/aarch64/boot.s`, `boot.rs`; `bin/chainboot/src/boot.s` |
| Core id | `libcpu::smp::CoreAffinity` (the `MPIDR_EL1` affinity fields, in PSCI `CPU_ON` layout); no logical index yet | `libs/cpu/src/arch/aarch64/smp.rs` |
| Kernel state | Only an init-phase flag per image, for `InitStateLock`; nothing ends the phase, and SMP keeps no lifecycle state (C13(a)) | `libs/kernel-state/src/lib.rs` |
| Kernel lock | `KERNEL_LOCK`, a ticket spinlock (`liblocking::TicketLock`) held by core index + 1: every kernel entry from a Thread (`current_el0_*`, the SVC path of `current_elx_synchronous`, `lower_aarch64_*`) and the idle-path interrupt take it once (`KernelLock::enter`) and release it on return; the idle loop releases it around `wfi`; each section on the `Nucleus` asserts this core holds it (`KernelLock::held`), and a core taking it again panics | `kernel/nucleus/src/main.rs`, `libs/locking/src/ticket.rs` |
| Per-core state | In each core's `CoreRecord`: the scheduling state and totals (`CoreState`: `current_thread`, `current_since_ticks`, `dispatched`, `dispatch_next`, `preempt_requested`, `interrupt_ticks`, `idle_ticks`, `fp_totals`), which the `Nucleus` reaches through its `this_core` field (`ThisCore`, by `TPIDR_EL1`), under the kernel lock; and, touched by their own core without the lock, `idling`, the programmed timer deadline and the FP entry guard's exit flag and toggle count | `kernel/nucleus/src/objects/core_record.rs`, `nucleus.rs`, `main.rs`, `timer.rs`, `objects/arch/fp_unit.rs` |
| Kernel stack | One 512 KiB trap stack per core in `SP_EL1`, mapped by Kickstart in slots after the nucleus's `__STACK_VIRT_BOTTOM` with guard pages; a per-core record (`nucleus::core_record`) in `TPIDR_EL1` on every core; the boot Thread runs EL1t on the init stack after the image, the secondaries EL1h on their trap stacks | `kernel/kickstart/src/cores.rs`, `paging.rs`, `el_switch.rs` |
| EL2 → EL1 | Kickstart runs at EL2 with the MMU off, builds the tables, releases the secondaries, then `el_switch` sets VBAR, MAIR, TCR (inner shareable), SCTLR and drops to EL1t; each secondary runs the same `el_switch` into EL1h | `kernel/kickstart/src/el_switch.rs`, `secondary.rs` |
| Secondary cores | Released by the start-method component at EL2; at EL1 each sets its bit in `ONLINE` and goes on into the nucleus's `nucleus_secondary_idle`, a stub that reserves one exception frame at the top of its trap stack and calls `secondary_main` (`TTBR0` walks and its timers off). It waits on `wfe` until the boot core has installed the interrupt controller (and run its own `init_cpu`), then under the kernel lock runs `init_cpu`, unmasks its own timer line and waits in the kernel's idle loop (`idle_until_runnable`) for a dispatcher on its own list, taking its IPI line and timer as kernel entries. The first Thread it selects (its root dispatcher Thread) is entered through the reserved frame and `__restore_context`, so `SP_EL1` is the stack's top while the Thread runs; the selection checks the Thread's translation against `TCR_EL1` with `EPD0` taken as clear, since the core turns TTBR0 walks back on as it installs that translation. The boot core waits for them for at most 1 s in `el1_entry` | `kernel/kickstart/src/secondary.rs`, `lib.rs`; `kernel/nucleus/src/main.rs` |
| TLB maintenance | Nucleus already uses broadcast forms (`tlbi vae1is`, `tlbi aside1is`, `dsb ish`); Kickstart's boot flush is local `tlbi vmalle1` | `objects/arch/aarch64_objects.rs`, `el_switch.rs` |
| Interrupt controller ABI | `IrqChipOps`: `init`, `claim` (on the calling CPU), `complete`, `mask`, `unmask`, `set_trigger`, `xlate`. No per-CPU init, no IPI, no routing | `libs/irqchip/src/lib.rs` |
| BCM2836 component | Drives core 0's local registers only; core 1–3 timer, mailbox and source registers are reserved fields | `drivers/irqchip-bcm2836/src/lib.rs` |
| GIC-400 component | Routes every SPI to the CPU interface of the core running `init` (from banked `GICD_ITARGETSR0-7`) | `drivers/irqchip-gicv2/src/lib.rs` |
| Scheduling ABI | A Scheduler has a core (its own core for a root, the parent's from `Adopt`); `Scheduler.Dispatch` `x6` must name it and the caller must run on it (`SchedulerKey::dispatch_on`); `ConfigureThread` binds only a Time of the Scheduler's core (the caller's for a Scheduler with no core yet); the Thread record's `cpu` is the running Thread's Scheduler's core; a Scheduler's record is written only on its core, with that core's totals; a Time belongs to the core that created it, and Transfer, Merge and Dispatch refuse another core's | [scheduling design](scheduling-design.md#decisions); `objects/scheduler.rs`, `objects/time.rs`, `api/scheduler.rs` |
| Per-core roots | Kickstart carves a boot Time (infinite) and a root Scheduler for every online core (`BootState::core_roots`; the boot core's at `BOOT_TIME` and `ROOT_SCHEDULER`, the others at the top of the boot table). The root dispatcher (`userspace/dispatcher`) has one Thread per online core, started with the core's index in `x0`: the boot core's is its first Thread, each other core's is configured on that core's root Scheduler and boot Time, and each runs its own core's Scheduler from its own queues (`DispatchInit::cores`, one record table per Scheduler). A root `BindDispatcher` for another core queues the dispatcher on that core's list and sends `IPI_RESCHEDULE`. Every managed Thread is still the boot core's, so the other cores' dispatchers wait with nothing dispatched. `smp-test` runs Threads on core 1's root Scheduler: entry, a cross-core Signal wakeup, parallel execution with core 1's timer ending a budget, and the per-core Transfer refusal | `kernel/kickstart/src/bootstrap.rs`, `drivers.rs`; `userspace/dispatcher`; `objects/nucleus.rs` |
| Run state | `Nucleus::activations` is one list per core, a Scheduler's dispatcher on its core's list; a core selects from its own (`local_activations`); an activation on its own list preempts its running Thread, one on another core's list sends that core `IPI_RESCHEDULE`, and only the target core updates its own `dispatched` (`settle_dispatched`). The dispatcher-timer table and the wait timeouts are tagged by core, and each core programs and expires only its own | `objects/nucleus.rs`, `objects/timers.rs`, `objects/completion.rs`, `main.rs` |
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
| QEMU `raspi3b`/`raspi4b` | 4 | spin table at the same addresses, written by QEMU's own boot stub (`hw/arm/raspi.c`, `write_smpboot64`) | Same as Pi 3 | QEMU starts every core at EL2, as the firmware does |

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
- A secondary core starts with its own EL2 registers unset. `libboot`'s `configure_el2`
  (`CNTHCTL_EL2`, `CNTVOFF_EL2`, `SCTLR_EL1`/`SCTLR_EL2`, `HCR_EL2`, `CPTR_EL2`) runs on
  every core, followed by `el_switch` with the same `TTBR0_EL1`, `TTBR1_EL1`, `TCR_EL1`,
  `MAIR_EL1`, `VBAR_EL1` and the core's own `SP_EL1` and `TPIDR_EL1`. Kickstart's
  `_secondary_entry` (`kernel/kickstart/src/secondary.rs`) finds the core's parameters by
  its affinity in a table Kickstart publishes before the release, for the spin table and
  PSCI alike, and runs at EL2 on the physical top of the core's trap stack, which is
  otherwise unused until the core is at EL1.
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
- Set/way maintenance is local to the core doing it, so the whole hierarchy (with the
  levels the cores share) is cleaned and invalidated once, by the boot core, before the
  release; a secondary walks only its own L1.
- What a secondary writes with caches on (`ONLINE`) sits in a cache line of its own: a
  dirty line written back later would overwrite whatever the boot core stored in the same
  line with its caches off after the release.
- A store that cores waiting in `wfe` look for is completed before the `sev`
  (`libcpu::send_event`: `dsb ish; sev`). Otherwise a waiter can wake, read the old
  value and wait again for an event that already came: the first Pi 4 run of `smp-test`
  lost its coherency round that way, which QEMU does not show.

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
table), which also holds the per-core run state: one `activations` list per core, and
one dispatcher-timer table whose entries carry their Scheduler's core, as wait timeouts
in the pending records carry the core of the waiting Thread's Scheduler. The existing scheduler-record
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
  before claiming.
- Per-core record fields are touched only by their own core, without the lock, or under
  the lock by another core only where noted (pending IPI bits are atomics).
- Kickstart keeps `IRQSafeNullLock`: it runs on one core with the MMU off, where an
  exclusive never completes.
- **It orders what userspace publishes through the kernel.** Its release (the `serving`
  store) and acquire (the `serving` load) order every access before and after them in
  program order, EL0 ones included. So a `Signal`/`Advance` and every observation of it
  (wakeup, satisfied wait, `Poll`, `Await`, `Read`) need no atomics of their own: each
  runs inside a kernel entry, and the producer's user writes before its entry happen
  before the consumer's user reads after its own ([contract, payload
  publication](capabilities-contract.md#notification-and-eventcount)). A fault frame the
  nucleus writes is read by its handler only after a kernel entry resumes it.
- **The one publication read without a kernel entry is a scheduler record.** Its sequence
  lock carries the ordering: the nucleus makes `sequence` odd, issues a release fence,
  writes the fields and stores the even `sequence` with release; a reader loads
  `sequence` with acquire, reads the fields, issues an acquire fence and reloads
  `sequence` ([scheduling design, publication](scheduling-design.md)). Both record kinds
  are tested with a writer and three reader threads on an Arm host, where the test fails
  without the two fences. The nucleus writes the records through its cacheable linear
  map; the ordering also needs the scheduler's own mapping of the pages to be normal
  cacheable memory, which nothing checks yet.

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
  numbered `LINE_MAILBOX0..3` in the component. Checked against the QA7 datasheet
  (`doc/bcm2836-peripherals.pdf`, rev 3.4): §4 and §4.8 for the addresses, §4.7 and
  §4.10 for the control and source bits, §3.3 for a mailbox interrupting while
  non-zero.
- **GIC-400 (Pi 4, Pi 5):** SGIs through `GICD_SGIR` (target list bits 23:16, INTID
  bits 3:0). SGI/PPI enables, priorities and the CPU interface (`GICC_CTLR`, `GICC_PMR`)
  are banked, so each core initialises its own.

The IPI kind travels in the per-core record (atomic bits); one hardware line or SGI is
enough. As built:

- `CoreRecord::pending_ipis` holds the kinds (`IPI_RESCHEDULE`, `IPI_STOP`, `IPI_HALT`)
  and `ipis_taken` counts the lines taken. A core finds another's record through
  `core_record::of(index)`: the records are one run, and each holds the core count.
- Sending (`send_ipis`): set the kind in each target's record with release, then one
  `send_ipi` of the target mask. Taking (`take_ipis`, in `handle_irq` when the claimed
  line is `ipi_line`): swap the kinds out with acquire. The claim acknowledged the line
  first, so a kind set after the swap raises it again.
- Reschedule is sent by `Nucleus::activate_on` when an activation or a top dispatcher's
  wakeup queues a dispatcher on another core's list; there `handle_irq` settles the
  core's own `dispatched` from the Schedulers (`Nucleus::settle_dispatched`) and sets its
  `preempt_requested`, so the exit path of an interrupt that stopped a Thread selects
  again. `send_ipis` is `core_record::send_ipis`. Halt parks.
- Stop is sent by `Thread.Retire` of a Thread running on another core (the Thread's
  `running_on`, set as a core enters it and cleared as it stops): the call marks it
  `retiring`, which `Dispatch` refuses, sends Stop once, and returns
  `InvokeOutcome::Retry`, which puts the caller back at its `svc` unchanged. The call
  runs again until the Thread is off the CPU, then retires it; between attempts the
  caller's core leaves the kernel, so the lock is free and it takes interrupts, and two
  cores retiring each other's Threads cannot wait for each other. The core taking Stop
  preempts a retiring current Thread even with nothing else to run, does not queue it
  again, and ends its dispatch with `EXITED`. A Thread that never takes interrupts is
  stopped only at its next kernel entry. An idle core taking Stop lets go of the
  AddressSpace it holds (see [Memory management](#memory-management)).

### Interrupt-controller ABI

`IrqChipOps` grows, in place (no backwards compatibility; `ABI_VERSION` stays 1):

- `ipi_line`: the line an IPI arrives on, on every core. BCM2836: mailbox 0 (local
  line 4); GIC: SGI 0. Its `claim` acknowledges it at the controller (BCM2836: clears
  the mailbox bits it read; GIC: writes `GICC_EOIR` with the whole `GICC_IAR` value,
  whose bits 12:10 hold an SGI's source CPU), so an IPI sent after the claim raises the
  line again.
- `init_cpu(cpu)`: called on each core once, after `init`, with its logical index. The
  component records the core's hardware identity (BCM2836: `MPIDR_EL1.Aff0`; GIC: the
  CPU interface bit its banked `GICD_ITARGETSR0`-`7` report) and unmasks the IPI line
  there; the GIC also enables the core's banked CPU interface and priority mask.
- `send_ipi(cpu_mask)`: bit `n` is logical CPU `n`; refused, signalling nobody, if the
  mask names a CPU that has not run `init_cpu`. BCM2836: a write-set of mailbox 0 of
  each target core; GIC: one `GICD_SGIR` write with the CPU target list.
- `claim`, `mask` and `unmask` of a core's own lines (its timers, mailboxes, SGIs and
  PPIs) act on the calling core: the nucleus unmasks its timer line on each core itself,
  as it already does on the boot core. The component's data (`static` in its `main.rs`)
  is shared, which the kernel lock covers.
- Per-line routing (`route(line, cpu)`) comes later; at first every device line goes to
  the boot core, as the GIC component already does.

### Timers

`CNTP` is per core and the counter is common, so the clock contract is unchanged. Each
core programs its own one-shot on every kernel exit (`reprogram_timer`) to the earliest
of its running Thread's dispatch limit, budget and slice, and the wait timeouts and
`Scheduler.ArmTimer` deadlines tagged with its core; its timer interrupt expires only
those (`Nucleus::expire_deadlines`). A wait timeout takes its core when it is set, from
the waiting Thread's Scheduler (this core for a Thread outside every Scheduler); a
dispatcher timer takes its Scheduler's core when it is armed.

### Memory management

- Translation updates are already break-before-make with `dsb ishst` and broadcast TLBI;
  nothing to add for remote TLBs on Arm.
- Any set/way cache maintenance is local only (`libs/cpu` cache notes); by-VA operations
  to PoC are broadcast. Audit the nucleus and drivers for set/way use.
- `TTBR0_EL1` is per core. An AddressSpace may be current on several cores at once
  (several of its Threads). As built: each AddressSpace has a `CoreUse` (`installed`,
  the cores whose `TTBR0_EL1` holds it or will at their next exit; `running`, the cores
  running one of its Threads), changed only by each core for its own bit, under the
  lock, wherever a translation is committed for the core (`Nucleus::enter_address_space`
  at Thread entry, PPC Call and Return, and `Activate`), with `running` cleared as the
  Thread stops; the core's `CoreState::installed_address_space` names the one it holds.
  `AddressSpace.Retire` and a root `PageTable.Unmap` run `quiesce_address_space`:
  `InvalidOperation` while it runs on another core (C8.1(a)), a Stop once to the other
  cores still holding it and `InvokeOutcome::Retry` until none does, then the ASID or
  root goes. An idle core taking Stop lets go of it and turns TTBR0 walks off
  (`ArchObjects::disable_translation_walks`); installing the next translation turns them
  on (`install_translation_context`, also for a secondary's first Thread). A core holds
  an AddressSpace idle only after its own dispatcher ran there: the dispatcher runs last
  before a core idles. Asynchronous retirement with quiescence detection is later work.

### FP/SIMD

The save is eager at the switch away (FP/SIMD design, "Multicore, later"), so no FP
state is left in another core's registers: the owner variable becomes per core and
nothing else changes.

### Panic, console and logging

The panic handler stops the other cores with the Halt IPI. Semihosting and the board
UART print from several cores, so a line must be written under a lock (the kernel lock in
the kernel; `libs/log`'s `static mut LOGGER` is set once at boot and is fine).

How it is done:

- The first core to panic claims the panic (`PANICKED_CORE`, its kernel-lock holder id)
  and sends `sev`.
- A core that panics after another parks at once, printing nothing. The same core
  panicking again still ends in `libmachine`'s re-entry guard.
- The panicking core takes the kernel lock (if it does not hold it: with
  `TicketLock::try_acquire` for at most 1 s, since the panic may be in the lock itself),
  then sends Halt to every other core and prints.
- The other cores park (interrupts masked, `wfe` forever):
  - a core taking interrupts is woken by its Halt IPI. The interrupt is a kernel entry,
    whose wait for the kernel lock gives up and parks once another core has panicked
    (`TicketLock::acquire_while`; the abandoned ticket leaves the lock taken for good,
    which nothing after a panic needs);
  - a secondary still waiting on `wfe` for the controller is woken by the `sev` and sees
    the flag;
  - the boot core's idle loop looks after each wake, before it takes the lock again.
  A core inside a kernel entry finishes it and parks at its next entry or idle wait.
- Every other kernel line is printed inside a kernel entry, so under the lock. The
  secondaries print nothing.

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
