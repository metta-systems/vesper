UART driver's client interface chosen without you
  The plan only says the PL011 driver "serves receive and transmit by interrupts". It never says how other programs send it bytes or get bytes back. The main agent mentioned its choice in one line and went ahead:
  - A shared page with two byte queues: one queue carries bytes to send, the other carries bytes received. The page is mapped into both the driver and its client.
  - Wake-up signals both ways: the client signals the driver's Notification when it queues bytes. The driver signals the client's Notification when new bytes arrive.
  - The client hands the driver these keys at fixed slots in the driver's key table before the driver starts.
  - It ruled out PPC calls into the driver and a driver-only self-test.

  It isn't recording this in your contract. Still, every future serial client will be built on it, so check that this is the interface you want before the test and the real boot path depend on it.

---

### 2. Finish the already-started interrupt-kernel foundation

Integrate the new exception carrier into Thread/main/bootstrap:

- EL1t execution on `SP_EL0`; shared high `SP_EL1` for traps.
- Thread-owned parked contexts, no retained exception-frame addresses.
- Normal handler unwind and exception return through the rewritten transient frame.
- Preserve the existing Notification/EventCount regressions.

---

extern "C" fn current_elx_irq(e: &mut ExceptionContext) {
// -- @todo
// let token = unsafe { &exception::asynchronous::IRQContext::new() };
// exception::asynchronous::irq_manager().handle_pending_irqs(token);
}

safe-mmio for MMIODerefWrapper?

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
