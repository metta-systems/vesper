# Capabilities decisions

This file is only a parking lot for **unanswered architectural or contract
questions**. Settled decisions belong in `doc/capabilities-contract.md` and, where
their reasoning is relevant, `doc/capabilities-design.md`.
Implementation, validation, and integration work belongs in
`doc/capabilities-implementation-plan.md`. Remove an item here once it is
resolved and recorded in its architectural document.

The [Future work](#future-work) section at the end is separate: ideas we might
or might not pursue depending on how the project develops. They need no
near-term decision, do not constrain current contracts or tests, and are not
plan items.

## D7: Protected Procedure Call (`Invocation`)


### Remaining PPC architectural decisions


- **Other architectural state:** what TLS, debug and other non-GPR/control
  state must be initialized, preserved or isolated before protected EL0
  execution? GPR/NZCV scrubbing alone is not complete state isolation.
- **Invocation creation through Retype:** should `AddressSpace.CreateInvocation`
  become Retype-style, like Thread creation (an inert object from Retype, made
  usable by the authority that owns it)? To be looked at; the maintainer
  leans towards switching.
- **Shared memory:** pointer/shared-memory rules.
- **Fault delivery refinements (D1):** the mechanism and its details are
  selected (see the contract). Open: a small per-AddressSpace fault queue that
  runs handlers one after another instead of treating a fault during a busy
  handler as unhandled; resuming with edited registers; more than one fault
  level per Thread.
- **Fault and lifecycle:** beyond the defined no-pop Return fault
  classification and the selected delivery mechanism, what are the call lifecycle,
  cancellation, Thread/AddressSpace teardown, partial-completion, and other
  nested/concurrent-call rules beyond the fixed depth limit?
- **Return to a live but unready source:** if the top record's saved source
  AddressSpace is still live but its root/ASID is missing or unencodable,
  is that a third Return fault, a `ReturnTargetRetired` fault, or an ordinary
  error? It currently surfaces as the preparation error without a pop.
- **Freezing the provisional PPC convention (D9):** the maintainer decides
  when it becomes frozen ABI. It works end-to-end through real Call/Return;
  no validation or plan item gates the freeze, and work should not be
  planned around it. "Provisional", "experimental" or "until field-tested"
  in the documents means this maintainer decision.
  - the common native target-body return convention (`extern "C"` body
    returning a `#[repr(C)]` two-`u64` struct in x0/x1).

## Other unanswered decisions

- **D1:** machine-local namespace and address-reservation conflicts for shared
  addresses (fbufs, common mappings), and which protection features each
  target backend must provide; resource-accounting and bounded-work mechanisms
  that hold even when a caller bypasses its libOS.
- **D2:** the bookkeeping, failure and coordination protocol between the
  managers of a libOS composition (multiple or hierarchical KeyMasters), and
  the completion handshake for retirement followed by background cleanup;
  invocation-versus-retirement ordering (after which point new operations and
  derivations cannot proceed) and how revoke completion is observed.
- **D2/D6:** selective invalidation of a branch of derived capabilities while
  the shared object remains live, including completion and safe-reuse rules.
- **D4:** per-kind semantic rights and the operation-to-rights matrix before
  bit assignments freeze; permitted attenuation of rights, extents, badges and
  budgets; badge width, zero-badge meaning, and mint/rebadge policy.
- **D4/bootstrap:** ownership of well-known `KeySlot` constants—shared object
  API versus bootstrap/libOS composition layout.
- **IRQ follow-up:** whether a Thread may bind one Notification for both
  PPC-server and interrupt waits (seL4 `TCB_BindNotification`).
- **D9:** ABI version and optional-operation discovery for separately built
  components: how a component learns the running kernel's ABI revision and
  supported operations, and what happens on a mismatch. Not needed while
  everything is rebuilt together from one tree.
- **D1/D6/D9 — DebugConsole byte transport (parked):** how a generally
  available console receives its bytes. (A) keep the pointer-based Write: the
  nucleus needs checked user-memory reads (caller-AddressSpace authorization,
  input snapshot, copy-fault recovery), which no other operation currently
  needs. (B) a register-inline operation, e.g. a length plus up to 40 bytes in
  `x3..x7` with client chunking, so the nucleus never reads user memory. Until
  this is decided the console stays a debug-only prototype. The hardware
  debugging aid's checked pointer reads (contract, DebugConsole debug-only
  exception) are temporary and do not answer this question.

## Future work

Not decisions to make now and not planned work; revisit only if the project
needs them.

- **Extended IPC outputs (`x1..x7`) and a per-thread IPC buffer:** returning
  more than two result words from a capability invocation, with message
  spill to a per-thread buffer. Today every ordinary invocation writes only
  `x0..x2` and preserves everything else (contract, ordinary control
  invocation baseline); adopting extended outputs would be a contract revision
  of that rule.
- **Temporal-VA protection for stale raw pointers:** kernel guarantees that an
  old raw pointer cannot reach replacement memory after its virtual address is
  legitimately reused — address quarantine, keeping revoked addresses
  inaccessible for a surviving Thread's lifetime, or stronger stale-pointer
  detection. Today the kernel gives no such guarantee and outside mechanisms
  own the prevention (contract, [protection requirements and
  boundaries](capabilities-contract.md#protection-requirements-and-boundaries)).
  Would need its own separately scoped design.
- **Six-word PPC result payload:** widening the PPC result from two to six
  `u64` words if component interfaces need more register-only results. Would
  need a coordinated change to the Return transport, wrappers and decoders.
- **64-bit allocation generations:** explore whether widening kernel
  allocation generations (`ObjectId`, pool slot metadata and pending-record
  generations) from 32 to 64 bits is feasible and worthwhile. A slot
  exhausting its generation is retired from reuse today; 64 bits would make
  that unreachable in practice, at the cost of larger identities in every
  capability entry, pool metadata record and pending record. Would need a
  measured look at those layouts and the `KeyEntry` payload size.
- **Continuing carved-object generations across re-carving:** instead of a
  fresh header starting at the first generation, investigate whether
  re-carving could read the previous header's generation and continue from
  it. It would need the old generation to survive reclamation (the bytes
  are not sanitized, or the generation is kept elsewhere), a rule for memory
  re-carved at a different offset or as a different kind, and a check that it
  adds protection beyond the retire-time bump and completed cleanup that the
  contract already relies on.
- **Kernel-published identity in `TPIDRRO_EL0`:** using the EL0 read-only,
  EL1-writable register to publish per-Thread identity to userspace, as Linux
  does for per-CPU data. Today it is always zero.
- **Driver probe/init calls:** Kickstart hands a driver everything through its init page and never calls into it. A device manager may need a probe/init part of the driver interface (asking a driver whether it can drive a device, or telling it to start or stop); look into what that would be and how it is called.
- **Per-client mailbox policy:** `mailbox_v1.property` applies one fixed tag whitelist to every client. Different rules per client (for example clock and power setters for `cprman` only, memory tags for `vcsm`) need caller identity through Invocation badges (D4), which would also replace identifying the caller by its exporter stack.
- **Count ranges on CopyDerive and Move:** an optional count that copies or moves a run of consecutive slots in one operation, like NOVA's `ctrl_pd` selector order. The framebuffer hand-off needs only two to four 2 MiB Frames, so single-slot CopyDerive is enough today; ranges would matter for hand-offs of hundreds of small Frames.
- **Keys in call results:** letting `Thread.Return` carry a key back to the caller, and letting `CallWithKey` lend more than one key. Today a call lends one key and results carry data only.
- **Resources named in `uses`:** declare the boot-image resources a component
  maps in its `imports.if` as `uses resource font;`, in place of the
  `resources = [...]` list of its manifest entry. Meddle's `uses` parser, AST
  and summary would carry the resource names (their order stays the index in
  `DriverInit::resources`), and image-build would read them from the imports
  file. Today the manifest list does the job.
- **Universal channel provisioning:** a general way to set up shared control
  pages and data areas between components, instead of per-interface wiring (the
  per-pair data areas Kickstart builds for Meddle `data` declarations, the
  interrupt-IN channel of the USB stack and the hand-wired serial channel are
  three cases of it). Would need its own design, for example a Meddle `channel`
  declaration shared by all of them, or client-lent Frames with a mapping window.
- **Releasing the stacks of a faulted Thread:** a Thread that faults inside a
  pooled call keeps its stack until its continuations are dropped. The
  resource manager is meant to clean up Threads stuck in the faulted state,
  which would also free those stacks; how it unwinds or retires them is not
  designed (see the [stack pools](capabilities-contract.md#stack-pools-maintainer-decision-2026-10-19) section of the contract).
- **Chainboot with cached RAM:** the `mmu` build of chainboot maps RAM as Normal
  non-cacheable with the data cache off, so atomic read-modify-write still does
  not complete on the Raspberry Pi's cores (exclusives need a global monitor the
  Pi does not have for uncached memory) and `build-chainboot` fails on any such
  instruction. Mapping RAM write-back cacheable with the data cache on would
  make atomics work, at the price of cache maintenance: clean the kernel image
  and invalidate the instruction cache before the jump; clean and invalidate the
  whole data cache by set/way before the MMU goes off, so that stale dirty lines
  cannot overwrite the kernel later; clean the mailbox buffer before each send
  and invalidate it after the answer, or keep the buffers in a non-cacheable
  region. Worth doing only if chainboot needs atomics; QEMU does not model
  caches, so the board is the test.
- **Arasan SDHCI for the card on the boards:** the Raspberry Pi 3 has two SD
  controllers, and the device tree says which one has the card (the enabled
  node whose pin group holds GPIO 48 to 53). The stock tree enables SDHOST and
  gives the Arasan to the Wi-Fi chip, so the `sdhci` driver serves
  `brcm,bcm2835-sdhci` only in `qemu` builds; a tree that enables the Arasan
  for the card instead (the `sdio` overlay) would need the driver to tell the
  card's node from the Wi-Fi chip's, for example by that pin group.
- **Zero-copy DMA for packet drivers:** a driver that moves a client's
  buffers by DMA (the `sdhci` driver's ADMA2 today, xHCI and network drivers
  later) does not know the physical addresses of the channel data pages its
  clients share with it, so the controller moves data through bounce pages of
  the driver's own memory and the CPU copies every byte once more between
  those and the client's buffers (as the DWC2 driver does). DMA then saves the
  wait on the controller, not the copy. Avoiding the copy needs packet-driver
  infrastructure that tells a driver the bus addresses of the pages it is
  given: Kickstart recording them for the channel pages it maps, client-lent
  Frames whose extent the driver reads (`Frame.GetAddress`), or a channel
  whose data pages the driver allocates from its own memory. It also needs the
  pages in the device's DMA window (the BCM2711's first gigabyte for EMMC2)
  and a rule for cache maintenance on pages two components map. Ties in with
  universal channel provisioning above.
- **The activity LED handed to its driver:** `mailbox_v1.set_led` picks the
  LED's firmware GPIO from the board revision code the firmware reports
  (`mailbox_impl::activity_led_pin`), so the mailbox component knows boards.
  Kickstart could instead read the device tree's `/leds` node
  (`libdevicetree`'s `DeviceTree::activity_led`) and hand the LED to whoever
  switches it through the driver init page: a firmware GPIO to the mailbox
  component, a SoC GPIO to the GPIO component, the Raspberry Pi 5's
  `brcm,brcmstb-gpio` pin to a driver for that block.
