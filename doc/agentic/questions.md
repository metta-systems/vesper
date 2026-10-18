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

---

+usb
sdcard
+compositor
"terminal" windows

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
