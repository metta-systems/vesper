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
