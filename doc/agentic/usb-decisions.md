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

## 2. How `usb-core` finds the xHCI controller (needed by USB6 slice 2 on the Pi 4)

`usb-core` imports `usb_hcd_v1` `via device compatible "brcm,bcm2708-usb"`, the DWC2
node. The Raspberry Pi 4's VL805 node has no compatible string (the xHCI driver is
loaded by node path), so in board-test `usb-core` would link nothing. The way an
import names one controller on one board and another on the next is not decided.
