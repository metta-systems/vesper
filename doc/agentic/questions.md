- [ ] seemingly AI decided we use process stacks, while it should be interrupt stacks for kernel state. Thread context should be stored in the TCB.
- [-] VSpace -> Container?
  - [ ] Component?
- [ ] pub asid: Option<u16>, should probably use Option<NonZero<u16>> in domain.rs
  - ASID 0 reserved for the kernel's own boot translation context
  - [ ] similarly for some other int-based optionals, where 0 is clearly an invalid value - find them
- [ ] "a per-domain IPC buffer receives anything beyond register capacity. The buffer's registered location and lifetime are part of domain creation (D5/D6 follow-up)." - does a per-Endpoint IPC buffer make more sense? How it is implemented?
- [ ] two mmio tests ignored -- running 2 tests
  test libs/mmio/src/lib.rs - MMIODerefWrapper<T> (line 42) ... ignored
  test libs/mmio/src/lib.rs - MMIODerefWrapper<T> (line 46) ... ignored
- [ ] Move tests from kernel/tests to individual libs?

---

- [-] Rename Kickstart to Ymir? - kickstarter is clearer (ymir can be internal name or leave it for libos initializer)
- [x] CSpace = KeyTable
- [-] our Domain shall be more like Protection Domain (= TCB + CSpace + VSpace), 
- [x] ObjectPool .meta is limited to 256 entires, which is wrong - the meta should be allocated together with the pool from the Untyped, this is where we know the actual capacity.
- [x] Extract testing bits from kickstart into a separate kicktest binary. (2026-09-24: kickstart is now a lib + real-boot bin — the e2e suite, Bounce fixture, and test helpers live in the separate kernel/kicktest crate reusing the shared boot path; `just test-capability-boot` builds/boots the kicktest image via `just build-kicktest`)
- [x] Make KeySlot::DEBUG_CONSOLE not be slot 127, install it somewhere else (the capability will be 127 though, so it's more natural)
- [x] "cannot parse the debug map for" - problem in kache
- [/] What's needed: quick-n-dirty higher-half mappings setup and kernel physical memory view setup (both in TTBR1), identity mapping for kickstart (in TTBR0).
  - `__kernel_start` to `__kernel_end` map at KERNEL_HIGH_BASE
  - 0 to phys_ram_size (from DTB) map at KERNEL_PHYS_WINDOW
  - `__kickstart_start` till `__kickstart_end` identity-map (no code/data split yet?)
- [x] Make some caps work - Untypeds, Domains, Buffers, what else?
- [x] Test out syscalls from EL0
- START FILLING IN CAPS
  - [x] untypeds
  - [x] kickstart context and domain

---

# Docs

What was created

`vesper/doc/object_types/` now contains a complete reference for every kind in the capability catalogue, grounded in the current implementation (`kernel/nucleus/src/objects/`, `kernel/nucleus/src/api/`, `libs/object/src/`) and the three contract docs:

- **`README.md`** — index, wire encoding, invocation model, and a dispatch diagram
- **`core/`** — `null`, `untyped`, `domain`, `key_table`, `time`, `endpoint`, `notification`, `event_count`, `reply`, `debug_console`
- **`arch/`** — `frame`, `page_table`, `vspace`, `asid_pool`, `asid`, `io_space`, `io_port`, `irq_handler`, `irq_control`

Each document follows the same structure: name/metadata table → purpose → user-level visible operations (wire schema, authority, results) → kernel-level implementation details (with small mermaid diagrams: retype transaction, retire teardown, KeyTable lookup precedence, Notification state machine, EventCount overflow flow, Frame.Map transaction, PageTable installation, ASIDPool.Assign, Endpoint rendezvous, dispatch) → sidenotes → TODOs → **final cross-reference section** against the 🧠 Vesper vault.

## Most important discrepancies flagged for your review

1. **KeyTable architecture** (`key_table.md`): vault describes a KeyNode directed graph with guarded page tables, radix/depth-limits, kernel CDT, and recursive revoke; current implementation is a flat 256-slot table, no CDT, Revoke rejected. Also 32-byte `KeyEntry` vs the vault's assumed 16 bytes/slot.
2. **Single address space** (`domain.md`, `vspace.md`, `frame.md`): vault SAS vision vs. selected per-Domain translation contexts (D1).
3. **Untyped split + device retyping** (`untyped.md`): vault expects Untyped→Untyped splitting and device-Untyped→Frame retyping; both are rejected today.
4. **ASID pools** (`asid_pool.md`): vault/seL4 imply Retype-creatable pools; current model is boot-provided-only.
5. **IRQ delivery** (`irq_handler.md`, `irq_control.md`): the entire interrupt HAL is an unchecked vault todo; Notification is ready as the target but nothing delivers to it.
6. **IPC surface** (`endpoint.md`): vault expects non-blocking Send/Recv variants and a `Yield` syscall — neither exists nor is contracted.

## Questions I skipped (per your instruction)

1. **Scope of "currently existing"** — I documented *all* catalogue kinds (including reserved/deferred ones like `VSpace`, `ASID`, `IOPort`) with explicit status lines, rather than only the active kinds. Trim if you wanted active-only.
2. **README index** — I added one; remove it if you want only the per-type files.
3. **Untyped→Untyped split** — treated as a TODO/discrepancy rather than a deliberate rejection; confirm intent.
4. **Vault phrasing "kernel creates Reply object"** — flagged in `reply.md` as a mechanism divergence (pending-record vs pooled object); needs your call.

No code was changed, so no build/test validation was run; the mermaid diagrams use only supported types (flowchart, stateDiagram, sequenceDiagram).


---

# Completed

- [x] Buildable
- [x] build all shit together into a binary
- [x] make separate kickstart section and start booting from kickstart
- [x] make early_print work
- [x] parse dtb
- [x] Print that we can invoke kernel function using a syscall from the kickstart (even it if runs at the same EL for now)
- [x] Enter kernel init in EL2 - this will be needed to set up kernel mappings
- [x] Print DTB
- [x] Print max RAM from DTB

- [x] Allocate a single key slot in a global domain struct
- [x] Fill it with capability to DebugConsole
- [x] Invoke DebugConsole.Write via syscall

- [x] Generate complete memory map from DTB, print it out

- [x] Print kernel covered area
- [x] Print KERNEL_HIGH_BASE
- [x] Print kernel mappings size and attribs
- [x] Print kickstart covered area
- [x] Print kickstart mappings size



Whatever kernel links must also be located in high-mem mapping, so we cannot share this code with kickstart at all!
This means it's probably sensible to build kernel as a separate ELF file linked entirely high, then merge it with the kickstart binary through specially-named sections; there should be no symbol resolution across two binaries, so the nucleus image is solely pulled via it's PHDRS (but we need to place the BSS which will be erased by the kickstart before turning the MMU on)

- [x] See gh:metta-systems/kernel-embed-prototype for an outline of this approach - copy it here and lets go.

---

A side finding, not fixed here: the device-region names Kickstart keeps in BOOT_INFO all point into the device tree by physical address. Anything that prints them after the MMU switch would crash the same way. Today only QEMU-only prints use them.

----

- A Time is a budget of CPU time, and each Time belongs to one core.                                  [-]
  - The scheduling rule says Transfer, Merge and Dispatch "reject a Time of another core". The main agent read "another core" as any core except the one making the call. Before, the check only required the two Times to be on the same core.
  - So a dispatcher running on core 0 can't hand budget to a Time that belongs to core 1. Each core's budget can only be refilled by code running on that core.
  - This shapes how the per-core boot Times and root dispatchers (the next items) can be funded.
  - Confirm this reading is what you meant, or ask for "both Times on the same core" instead.

---


- Image size per board set: an rpi4+rpi5 image still carries every driver, including the RPi3 interrupt controller.
- Devicetree pointer: the kernel entry still takes it as a 32-bit value. That's fine while the firmware puts the tree below 4 GB.

---

Still needed for sound on the Pi 4:
- gpio needs brcm,bcm2711-gpio.
- cprman needs brcm,bcm2711-cprman.
+ The jack needs a pwm1 overlay.

write meddle format document, updated vs the old one

stop doing this:
# The spi component imports the log server's log_v1, which this image does not
# bundle: the import stays unlinked and its trace goes to semihosting.
interfaces = ["../../../userspace/log-server/interfaces"]
that's what include paths are for...

Still open from SPI work:
- Chip-select ownership: any spi_v1 client can address any chip select on the bus.
- DMA for pixels: each 4 KiB chunk is one polled call, and a full frame takes 47–120 ms.
- Programs owning GPIO pins: should a program without a node be able to own pins, for example through its manifest entry?

---

- add `cargo mutants` support for kernel tests??

---

Next is the read-only filesystem over `block_v1`: should it be exFAT or ext4?
(Can fit entire WAD into the bootimage for now as `[[resource]]`)

---

`Mobj -> MapObject` in Room

Filesystem ops to Platform.

---

Room's config file is never loaded or saved
- m_config is Doom's settings system. Each setting, such as a key binding, a chat macro or the music command, is linked by name to a variable in the game.
- In this port the code that reads and writes the .cfg file was left out (the upstream C code switches it off too). M_LoadDefaults only works out the filenames and prints them.
- So all ~150 M_BindVariable calls record where each setting lives, but nothing ever fills those variables. Settings keep their built-in defaults, and changes are never saved.
- The current conversion just tidies up the types, which is fine. Don't count on settings like key bindings persisting on the host or on Vesper until someone writes the file loading and saving.

---

Compositor:

- Clients: nothing receives the routed events. They only go to the trace. Real clients and per-client channels are P-UI-COMPOSITOR-3 and P-UI-COMPOSITOR-4. The three windows are hardcoded.
- Pointer events: nothing sends events to clients yet (the P-UI-COMPOSITOR-4 stage).
- Mouse: the mouse path is still untested, in QEMU and on hardware.
- Cursor motion: I haven't injected pointer motion in QEMU to confirm it moves. That is the stated P-UI-COMPOSITOR-7 exit check.
- Real board: next step is `just _boot-compositor-test`, which includes the framebuffer, and a production image build on the Pi.

---

Most important: stale against the current contract

1. Retype wire diagram is wrong. libs/syscall/src/lib.rs:24-27 says x2 = obj_type, x3 = dest_captbl, x4 = dest_slot_start, x5 = count. The contract (doc/capabilities-contract.md ~line 336) defines x2 kind, x3 size_bits, x4 count, x5 destination-table key, x6 first slot, x7 rights. Replace the diagram with that schema.
2. CopyDerive diagram is stale. libs/syscall/src/lib.rs:8 shows x0 = src_captbl, x3 = dst_captbl. The contract's status comment at lines 37-40 of the same file says CopyDerive uses packed keys in x0/x2/x3. The two disagree.
4. Legacy catalogue naming in code. ObjectType::THREAD, KEY_TABLE, NOTIFICATION and similar constants are used in libs/object/src/object_type.rs and across kernel/nucleus/src/objects/*.rs, kernel/tests/*. The contract says to use the CoreType IDs. ObjectType is still the one-byte wire wrapper, so the question is whether its per-kind associated constants should be CoreType values. This is a naming migration to confirm with you, not a clear bug.
6. Process language in user docs. doc/object_types/ must have no "provisional", decision IDs, "contract" or "selected". I found doc/object_types/core/thread.md:32 ("provisioned", a sentence the skill calls out as process-like), and doc/object_types/README.md:138 which names the old CapabilityInvalidated status. Most of the other hits were false positives.

---

Medium: dubious or contradictory comments

- libs/object/src/key_table.rs:40: "Every thread has cap to own captbl here - or rather to KeyMaster". This is the old captbl naming plus an unresolved KeyMaster framing (D2/D4). Rewrite it to say what SELF_KEYTABLE is.
- libs/object/src/key_table.rs:223: "the legacy _captbl argument is unused". The contract has no _captbl argument. Remove the argument-level note, or fix the signature, once you decide whether revoke keeps a table key. The kernel rejects the operation anyway.
- libs/object/src/key_table.rs:139, 236 and libs/syscall/src/lib.rs:290: "provisional" rights and transport. These match the open D9 and D4 decisions, so they are correct as they stand.
- doc/capabilities-contract.md (the Scheduling records paragraph, around line 446): "Scheduler capability derivation is supported for planned hierarchical schedulers" appears twice in a row. The second copy is a duplicate, and "supported" is ambiguous next to "planned".


- The "explain why" rewrite was only partial. I removed the status tags and repaired the sentences around them, but I didn't rewrite each comment into why-style prose. That is a larger pass, and some comments are design intent you asked me to keep, so I'd rather not do it blind. I can take it file by file if you want.
- Some test function names still contain "selected", for example event_count_wait_infinite_is_the_selected_encoding in libs/object/tests/support/notification.rs and thread_operations_match_selected_wire_ids in libs/object/tests/object_type.rs. These are identifiers, not comments, so I left them.
- key_table.rs:337 still says "guarded object-identity foundation". It names a dependency rather than a phase, so I left it.

---

Missing caps 

IOSpace
IOPort
ASIDControl

---

DESIGN CHANGE: As explained [here](https://sel4.discourse.group/t/why-do-cnodes-have-guard-bits/49/5?utm_source=chatgpt.com) the KeyTable should not have a SELF_KEYTABLE slot by default and it shouldn't be required either.

This will require some changes in the API for AS.CreateInvocation and others.

---

+usb
sdcard
+compositor
"terminal" windows

SMP
kernel runtime upgrade?
is virtio-input needed for qemu??
isochronous scheduling

---

move AS first in the arch key numbers

---

### 2. Finish the already-started interrupt-kernel foundation

Integrate the new exception carrier into Thread/main/bootstrap:

- EL1t execution on `SP_EL0`; shared high `SP_EL1` for traps.
- Thread-owned parked contexts, no retained exception-frame addresses.
- Normal handler unwind and exception return through the rewritten transient frame.
- Preserve the existing Notification/EventCount regressions.

---

No split end-of-interrupt. The deactivate register exists in its register block, but nothing writes it or turns on the mode that uses it. So I dropped the planned "drop priority on delivery, deactivate on driver ack" scheme. The GIC uses the same protocol as the BCM controller instead: mask the line and end the interrupt on delivery, unmask when the driver acknowledges. The nucleus has no GIC special case.

Assumes Secure-world semantics. Its setup() picks group configuration as if running Secure. The component also sets distributor control bit 0, which is the Group 1 enable in the Non-secure view. It always acknowledges and ends interrupts through the registers that serve Non-secure Group 1, where the Pi firmware puts every interrupt. QEMU's raspi4b GIC has no Security Extensions, so this is unverified on real RPi4/5 hardware.

Upstream bug to avoid: set_interrupt_priority overwrites the priorities of the three neighbouring interrupts, so the component never calls it.

---

What arm-gic (0.10.0) covers and what it doesn't
- Fits: it is no_std with no statics, so it works inside the position-independent component. It covers the operations we need: acknowledge, end-of-interrupt, enable/disable, trigger mode and priority mask.
- No split end-of-interrupt. The deactivate register exists in its register block, but nothing writes it or turns on the mode that uses it. So I dropped the planned "drop priority on delivery, deactivate on driver ack" scheme. The GIC uses the same protocol as the BCM controller instead: mask the line and end the interrupt on delivery, unmask when the driver acknowledges. The nucleus has no GIC special case.
- Assumes Secure-world semantics. Its setup() picks group configuration as if running Secure. The component also sets distributor control bit 0, which is the Group 1 enable in the Non-secure view. It always acknowledges and ends interrupts through the registers that serve Non-secure Group 1, where the Pi firmware puts every interrupt. QEMU's raspi4b GIC has no Security Extensions, so this is unverified on real RPi4/5 hardware.
- Upstream bug to avoid: set_interrupt_priority overwrites the priorities of the three neighbouring interrupts, so the component never calls it.

What changed
- New component: drivers/irqchip-gicv2 handles arm,gic-400 and arm,cortex-a15-gic. Lines are GIC interrupt IDs; the RPi4 timer <1 14 0xf08> decodes to 30. The position-independent build is 676 bytes of code with 7 relocations, all R_AARCH64_RELATIVE.
- Host tests: 7 tests drive typed register_structs! views of the distributor and CPU interface, size-checked against arm-gic's own register blocks.
- Bundling: it is listed in kernel/kickstart/image.toml and privileged_components.
- New dependency: arm-gic in workspace dependencies; it brings in safe-mmio, zerocopy and arm-sysregs.
- Justfile: _run-test-kernel takes a board argument, and the new test-preempt-rpi4 (part of just test) runs the same test kernel on raspi4b. The GICv2 tests are in test-platform-host and clippy-platform-host.
- Docs: doc/object_types/arch/irq_handler.md lists both components and the single delivery protocol.

Verification: fmt-check, full just clippy and audit-fp-simd pass, as do all test suites. preempt-test passes on raspi3b and raspi4b; on raspi4b the boot log shows "physical timer line 30", and both EL1t spinners and the EL0 spinner were preempted. All 33 platform host tests pass.

---

Q1 is still open: how device-tree ranges become device Untypeds. An Untyped has to be a power-of-two size aligned to that size. Device-tree ranges aren't, and some nodes share a page.
- (a) Round each enabled node's range out to whole 4 KiB pages, merge pages that several nodes share, and split each run into the largest aligned power-of-two blocks. Any page touching a range the kernel drives is withheld whole. No two Untypeds overlap. I recommend this.
- (b) One device Untyped per node, rounded up to a single power-of-two block. Nodes whose blocks overlap would still need merging.

---

A crash inside a called component kills the caller's thread
- In Vesper, a cross-component call keeps running on the caller's own thread inside the other component.
- If the code there faults, the fault goes to the fault handler of the component that was called. That handler has three choices: retry, skip, or terminate.
- There's no fourth choice to undo the call and hand the caller an error. If the handler can't fix the fault, terminating stops the whole thread. The caller never gets its call back, so a buggy component can take down the threads that call into it.
- The contract text says a handler "may still choose" to unwind to the caller, but no such action exists yet.
- Consider whether an "abort the call with an error" action belongs on the plan, and fix that contract wording.

---

# Supposedly fixed

> Notifications and EventCounts take no memory from the region; each takes a slot in a kernel pool created at boot. A PageTable also takes a slot in the PageTable pool.


> Threads are created by the boot code; Untyped.Retype cannot create them.

---

`Untyped.Retype` cannot create AddressSpaces.

---

a finite sum reaching `u64::MAX`,  - currently in Time, but it should just saturate?

---

Meddler codegen should use minijinja templates instead of manually written code.
