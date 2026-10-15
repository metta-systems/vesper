Acaps and upcall threads

I've confirmed main has tcap-based async capabilities with no brands, while master adds acaps and upcall threads with only lingering comments referencing brands. Next I'll read master's acap code and the original brands paper for more context.

In the capability-table kernel (main, the same kernel ppos and beta are built on), brands were replaced by asynchronous send/receive endpoint capabilities plus temporal capabilities (tcaps). This is from the capinv.c and hw.h source I read earlier:

- CAP_ARCV, the asynchronous receive endpoint.
  - It is bound to exactly one thread, which waits on it with a receive call: it blocks, or takes pending notifications in a batch (cap_arcv_op).
  - It carries the receiving thread's tcap, which gives its priority and time budget.
  - It links to a parent scheduler thread, which notify_parent informs about the receiver's activity.
- CAP_ASND, the asynchronous send endpoint.
  - It names one receive endpoint, and sending through it activates the receiving thread.
  - The kernel (asnd_process) compares the sender's and receiver's tcaps. It switches to the receiver immediately if it should preempt, and otherwise leaves the event pending for it.
  - If the receiver lives on another core, the send becomes an IPI (cos_cap_send_ipi).
- Hardware interrupts reuse the same path.
  - A CAP_HW capability holds a bitmap of IRQ lines it may attach.
  - HW_ATTACH(irq, arcv) builds a kernel-held send endpoint for that IRQ (hw_asnd_caps[irq]), so **an interrupt is just a send from the kernel** (cap_hw_asnd).
  - Preemption follows the same tcap comparison as a software send.
  - There are no trigger, routing or acknowledge operations.

So where **a brand was an event context backed by a pool of upcall threads**, the current design gives each event source one receive endpoint owned by exactly one thread. The interrupt line is attached to that endpoint, and **the tcap attached to it decides urgency**.

> A brand in Composite is itself a struct thread flagged with THD_STATE_BRAND, which directly informs your BrandedThread idea.

Brands lived from 2007 to 2014. Acaps replaced them in 2013–2014, and both were superseded by the capability-table kernel. All of this is from the Composite source and commit history.

Timeline

┌───────────────┬─────────────────────────────────────┬───────────────────────────────────────────────────────────────────────────────┐
│    Period     │              Mechanism              │                                   Evidence                                    │
├───────────────┼─────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────┤
│ 2007-10-02    │ Brands introduced                   │ Commit 04a234c3: "Primitive branding support"                                 │
├───────────────┼─────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────┤
│ 2008          │ Timer, network and                  │ fp_rr scheduler "using timer brands" (2008-04-07); network brands (2008-05)   │
│               │ scheduler-notification brands       │                                                                               │
├───────────────┼─────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────┤
│ 2013-06-18 →  │ Acaps built                         │ Commits "Acap implementation in process" and "Added ainv_wait … based on      │
│ 07-03         │                                     │ brand_ implementation"                                                        │
├───────────────┼─────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────┤
│ 2014-01-14    │ Brands deleted                      │ Commit 53c8e578: "Removed brand mechanism completely… The term 'brand' is now │
│               │                                     │  behind us"                                                                   │
├───────────────┼─────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────┤
│ 2014 → now    │ Capability-table kernel (the ppos   │ Asynchronous send/receive endpoint capabilities (CAP_ASND/CAP_ARCV) plus      │
│               │ branch, now main)                   │ temporal capabilities (tcaps), and an IRQ-attach capability (CAP_HW)          │
└───────────────┴─────────────────────────────────────┴───────────────────────────────────────────────────────────────────────────────┘

## Brands (kernel code as of 2013-04, before the acap work)

- A brand is a thread descriptor. It is a struct thread flagged THD_STATE_BRAND. It holds no running context of its own; it is the event's identity.
- It records where events land. COS_BRAND_CREATE copies the creating thread's invocation stack, cut off at a target component on that call path, so the brand records which component handles its events.
- It has a scheduling identity. It copies the creator's position in the scheduler hierarchy (copy_sched_info) and keeps a pending-event count.
- Handler threads are added to it. COS_BRAND_ADD_THD links upcall threads (THD_STATE_UPCALL). In this version the kernel accepts only one per brand.
- Hardware wiring is ad hoc. cos_syscall_brand_wire(brand, COS_HW_TIMER | COS_HW_NET, port) can wire a brand only to the timer or a network port, not to arbitrary IRQ lines.
- Activation preempts by priority. A hardware event or cos_brand_upcall calls brand_next_thread. That switches to the upcall thread immediately if the scheduler ranks the brand higher than the running thread; otherwise it bumps the pending count. The upcall thread waits with brand_wait, and an upcall can be passed on to an upstream component ("tailcall").

## Acaps, which replaced brands

- The brand thread is gone. An acap is a pair of capabilities: a client acap in the sending component and a server acap in the receiving component, wired together and created by an acap manager component.
- One receiver per server acap. Each server acap is bound to exactly one upcall thread, so the event's scheduling identity is that thread's.
- The send right is distributable. Any component holding the client acap can send with ainv_send, optionally restricted to an owner thread. The receiver waits with ainv_wait.
- Same preemption logic as brands. ainv_next_thread either switches to the upcall thread or counts the event as pending.
- Same narrow hardware wiring. acap_wire still covers only timer and network.
- Cross-core sends become IPIs, added in 2014.

> The difference in one line: a brand was a thread-shaped event principal holding handler threads. An acap splits that into a distributable send right and a receive endpoint bound to one thread, with the scheduling identity coming from that thread.

The capability-table kernel then made both halves real capability-table entries (CAP_ASND/CAP_ARCV), moved priority and budget into tcaps, and generalized hardware wiring to "attach this IRQ line to this receive endpoint".

What this means for your BrandedThread idea: "a thread plus a brand" is essentially Composite's 2007 design, and Composite moved away from it. Every later step separated the sender's right from the receiving thread and attached the scheduling decision to the receiver.

The vesper analogues are close:
- A Notification already corresponds to the receive endpoint, and holding a key to signal it is the send right.
- The open question is mainly whether a Notification can be bound to a Thread so that a signal can preempt immediately (Composite's tcap check).
- IRQHandler would then just attach a line to that Notification and own mask/ack, which is arch-specific. Brand (core) wouldn't need to duplicate any of it.

seL4's IRQHandler, plus its Notification-to-thread binding (TCB_BindNotification), is the next thing to check against this. Shall I continue there?

## Nemesis

 From memory, not checked against the source: the privileged part comes from the driver.

      - Installing a stub: a device driver calls the IntAlloc interface with the IRQ line, a stub function and a state pointer. That interface is served by the privileged Nemesis system domain. It writes the pair into the kernel's per-IRQ dispatch table, which lives in the NTSC (the Nemesis Trusted Supervisor Code, its minimal kernel).
      - Running a stub: on an interrupt, the NTSC jumps straight to that stub, in supervisor mode with interrupts disabled. Nemesis is a single address space, so the driver's code is directly callable from the kernel.
      - What a stub does: usually it masks or acknowledges the line at the PIC and sends an event to the driver domain. The driver's own thread does the real work later and unmasks the line. There is also a generic default stub that does only that.

      So in Nemesis:

      - The NTSC provides only the dispatch table and the event-send primitive.
      - The privileged interrupt code is the driver's own, installed at run time through an interface only trusted domains can reach.
      - Isolation rests on trusting the drivers, not on hardware protection.

      Compared with vesper:

      - Vesper's privileged interrupt-controller (IC) components are a similar "privileged code that isn't part of the kernel" idea. But there is one per controller, chosen at boot by kickstart, not one per device installed by drivers at run time.
      - Device drivers stay at EL0 and get only an IRQHandler plus Notification, closer to seL4.
      - In other words, vesper keeps the Nemesis-style stub only for controller operations (claim, mask, EOI). The "send event to driver" step is generic nucleus code.

## Recommendation

 - No Brand object, no BrandedThread. Every role of a brand already has an owner:
   pending events → Notification; handler → waiting/bound Thread; sched identity → D5
   Scheduler records (seL4 MCS SC-on-notification / Composite tcap analogue); IPI target →
   kernel IPI when a Notification's waiter is on another core (Composite asnd, seL4 SMP).
   BrandedThread would recreate Composite's 2007 design and duplicate Notification.
 - IRQHandler = line authority: SetNotification(ntfn[badge]), Clear, Ack (unmask),
   SetTrigger. IRQControl = Issue(specifier → line) via the component's xlate.
 - Move IRQHandler/IRQControl to core (as seL4): semantics are generic; the IC-specific
   part is already behind the runtime IrqChipOps table, not compile-time arch code.
 - Optional later: Thread.BindNotification (seL4-style) if PPC servers need IRQ + call in
   one wait; decide with the D5 Scheduler event summaries.
