# USB open decisions

Parking lot for USB decisions the maintainer has not answered yet. Each item
names the phase that needs it. Do not assume an answer; record one only when the
maintainer gives it explicitly. Answered decisions move to the design doc's status
section, and tasks stay in [usb-implementation-plan.md](usb-implementation-plan.md).

The input event format (record, channel granularity, timestamps, output direction)
is tracked in [input-decisions.md](input-decisions.md), not here.

## 1. `usb-hid` device tree link (needed by USB4)

The USB core has no device tree node of its own, so the `uses` declarations of
`usb-hid` need a link chain. `drivers/usb-hid/interfaces/imports.if` is comment-only
until this is answered.

## 2. Scope beyond RPi3 (needed before any RPi4 work)

RPi4's USB-A ports sit on a VL805 xHCI behind PCIe. All work so far targets the
RPi3 DWC2 controller. The xHCI follow-up is recorded in the design doc. The question
is whether to plan it now, and whether the `usb_hcd_v1` interface must stay neutral
for it. The DMA decision above applies to xHCI rings too.

## 3. Who opens a claimed interface's interrupt pipe (needed by USB4)

An interrupt pipe's reports travel on the io-channel of the client that opened the
pipe, and a pipe's keys call the controller on the stack of that client's pair, which
only one Thread can use at a time. So a pipe opened by `usb-core` cannot be handed to
`hid` as keys, and its reports would arrive on `usb-core`'s channel. Options:

- **A. `hid` opens its own pipe (recommended).** `hid` imports `usb_hcd_v1` too, as
  the design's component table already says. `usb_v1.interrupt_pipe(claim)` returns what
  `hid` needs to open it: the device address, the endpoint address, the speed, the
  packet size, the interval and the route. Reports then arrive on `hid`'s own channel,
  bypassing `usb-core`. The cost: the controller opens a pipe for any importer to any
  endpoint, so the claim is a convention until the controller can check one.
- **B. `usb-core` opens it and relays the reports** over a second channel to `hid`.
  The authority stays with `usb-core`, at the price of a copy and a hop for every
  report.
- **C. Make delegated keys safe**: the controller mints the keys on the stack of the
  component that will use them. That needs the controller to know the holder, which
  the kernel does not tell it.
