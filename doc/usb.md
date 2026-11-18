# USB hardware notes

Controller behaviour the USB drivers depend on or work around, with the
evidence behind each. The architecture is in
[agentic/usb-design.md](agentic/usb-design.md).

## VIA VL805: no interrupts after ERDP is written high word first

The Raspberry Pi 4's USB-A ports sit on a VIA VL805 xHCI controller
(`1106:3483`) behind the BCM2711 PCIe bridge.

### Symptom

An interrupt IN endpoint completes its first transfer, then seems to go
quiet. Reports arrive in bursts, two seconds apart, whatever the typing
speed. The controller is not idle: its transfer events are in the event
ring, but no MSI comes for them, and `IMAN` reads `0x2` (enabled, nothing
pending). They are read only when a command's wait drains the ring. On the
Raspberry Pi 4 that was the watchdog's commands, every 2 s.

### Cause

The driver moves the Event Ring Dequeue Pointer (`ERDP`) past the events it
read, and writes 1 to its Event Handler Busy bit (bit 3 of the low word) to
clear it. The controller raises no interrupt while Event Handler Busy is set
(xHCI §4.17.2, §5.5.2.3.3).

xHCI §5.1, Register Conventions, has software write a 64-bit register low
word first, then high. Linux's `xhci_write_64` did so. The driver wrote
`ERDP`'s high word first. Written that way, the VL805 kept Event Handler
Busy set: it delivered no MSI for new events until a command's wait drained
the ring. With the low word written first, every completion comes on its MSI,
at once: a mouse report every 8 ms, keys as they are pressed.

No public VL805 documentation says which half of a 64-bit register it takes
the write on; the order above is what the board showed. Other controllers
differ: a Synopsys controller needs `ERSTBA` high word first in 32-bit mode
([LKML, 2024](https://lkml.iu.edu/hypermail/linux/kernel/2405.2/06971.html)),
a quirk Linux keeps off by default.

### What looked like a TRB read-ahead bug

Before the cause was found, a Stop Endpoint and Set TR Dequeue Pointer to the
waiting TRB, then the doorbell, made the next report arrive at once. That was
because the commands' waits drained the event ring, not because the endpoint
had stalled. The VL805 has a real TRB read-ahead bug, but a different one:

- [raspberrypi/linux#4685](https://github.com/raspberrypi/linux/issues/4685)
  and its fix,
  ["usb: xhci: add VLI_TRB_CACHE_BUG quirk"](https://github.com/raspberrypi/linux/commit/152ead29061b1c18a96255a187fd72d4843a783a)
  (December 2021): after a transfer TRB the VL805 also reads the next 64
  bytes, untagged by endpoint. At the end of a ring segment that reaches the
  first TRBs of the next page, possibly another endpoint's ring, and a later
  read there can use the stale copy. Ringing the doorbell again recovered the
  stalled endpoint in that report.
- Linux now gives each transfer ring segment a dummy page after it
  (`XHCI_TRB_OVERFETCH`,
  ["usb: xhci: Enable the TRB overfetch quirk on VIA VL805"](https://lkml.indiana.edu/hypermail/linux/kernel/2501.0/05457.html),
  January 2025). The Raspberry Pi kernel first shortened each segment by four
  TRBs instead.

Vesper's interrupt rings are 64 TRBs at the start of a page whose tail is the
report buffer, so a read past a ring's TRBs reaches no other ring.

Other VL805 quirks in the Raspberry Pi kernel's
[`xhci-pci.c`](https://raw.githubusercontent.com/raspberrypi/linux/rpi-6.6.y/drivers/usb/host/xhci-pci.c),
which Vesper's paths do not yet reach:

- `XHCI_EP_CTX_BROKEN_DCS`: a stale dequeue cycle state in the output
  context after a halt.
- `XHCI_AVOID_DQ_ON_LINK`: never move a dequeue pointer onto a Link TRB. The
  "can't handle a SET_DEQUEUE_POINTER command on a stopped ring" remark in
  #4685 is about this case.
- `XHCI_VLI_SS_BULK_OUT_BUG`: SuperSpeed bulk OUT corruption.
- `XHCI_VLI_HUB_TT_QUIRK`: on firmware before `0x0138C0`, a wait for MFINDEX
  to leave microframe 7 before queueing to a full-speed device behind the
  integrated hub.

### What Vesper does

`drivers/usb-hcd-xhci` writes `ERDP` low word first, at bring-up and after
each drain. It sets interrupter 0's moderation interval (`IMOD`) to 40 µs, as
Linux does, whatever the controller's firmware left there, and traces the
value it found. Each Normal TRB goes in the slot after the last one, and the
endpoint's doorbell is rung.

A watchdog on the service Thread runs after 2 s without an MSI while an
interrupt pipe has a TRB out. It drains any events in the ring, as an MSI
would, and traces that it found some, with back-off. A device with nothing to
report keeps its TRB out, so the watchdog runs every 2 s while every device is
quiet.

In a `debug_kernel` image (`just boot-board-test debug_kernel`) the driver
traces an `xhci: debug:` summary every 10 s: the MSIs and the events read on
them, the events read in command waits, the watchdog runs, the reports handed
on and how many were all zeros, the events by kind, slot, endpoint and
completion code, and `IMAN` and `ERDP`.
