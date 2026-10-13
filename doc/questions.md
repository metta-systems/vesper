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

---

`Untyped.Retype` cannot create AddressSpaces.

---

a finite sum reaching `u64::MAX`,  - currently in Time, but it should just saturate?

---

A PageTable also takes a slot in the kernel PageTable pool. - do we need this pool?

---

Meddler codegen should use minijinja templates instead of manually written code.

---

Not done: capability transfer (2b)

I did not implement it because its schema is unanswered, and I won't guess a kernel ABI. These are the questions, with my recommendation first:

1. How the caller names the keys it passes: a per-function declaration in the .if file, compiled into a key mask when the Invocation is created (recommended), or a flag set at call time.
2. Where the callee receives them: the kernel picks a free slot in the callee's table and puts its index in the argument word (recommended), or a fixed slot block.
3. How long they live: borrowed until the call returns, with the kernel deleting the entries afterwards (recommended), or kept.
4. Rights and kinds: the caller's requested rights intersected with the key's, and only the existing CopyDerive kinds (KeyTable, Frame, DebugConsole).
5. Return: capabilities in results, now or later. I'd leave it for later.

Once these are settled, fb can Retype the framebuffer's Frames into the client's table using a table key the client passes. That is the last piece before the test shell.
