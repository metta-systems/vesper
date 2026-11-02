# USB open decisions

Parking lot for USB decisions the maintainer has not answered yet. Each item
names the phase that needs it. Do not assume an answer; record one only when the
maintainer gives it explicitly. Answered decisions move to the design doc's status
section, and tasks stay in [usb-implementation-plan.md](usb-implementation-plan.md).

The input event format (record, channel granularity, timestamps, output direction)
is tracked in [input-decisions.md](input-decisions.md), not here.

## 1. `usb-hid` device tree link (needed by USB4)

The USB core has no device tree node of its own, so the `uses` declarations of
`usb-hid` need a link chain. As built (not decided): `usb_v1` is linked `via component
usb_core`, the manifest name of the core, and `usb_hcd_v1` by the controller's
compatible string, as in `usb-core`, so `usb-hid` needs no node. This item stays open
until the maintainer confirms that link or names another.

## 2. Scope beyond RPi3 (needed before any RPi4 work)

RPi4's USB-A ports sit on a VL805 xHCI behind PCIe. All work so far targets the
RPi3 DWC2 controller. The xHCI follow-up is recorded in the design doc. The question
is whether to plan it now, and whether the `usb_hcd_v1` interface must stay neutral
for it. The DMA decision above applies to xHCI rings too.
