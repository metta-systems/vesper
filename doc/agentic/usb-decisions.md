# USB open decisions

Parking lot for USB decisions the maintainer has not answered yet. Each item
names the phase that needs it. Do not assume an answer; record one only when the
maintainer gives it explicitly. Answered decisions move to the design doc's status
section, and tasks stay in [usb-implementation-plan.md](usb-implementation-plan.md).

## 1. Pipe authority (needed by P3)

Whether a pipe handle is an Invocation key from the host controller, or a number
checked against caller identity by `usbd`. `interrupt_pipe` hands the HCD handle to
`hid`, which makes this decision concrete. The mailbox work currently relies on
SP-based identity until badges exist.

## 2. Hub location (needed by P3)

The hub driver sits inside `usbd` for v1, as the design recommends, or in its own
component. Class drivers other than the hub need no change either way.

## 3. Input event format (needed by P4)

The io-channel packet layout for key and pointer events: keycode or scancode, and
where keymaps and autorepeat live. `input_v1` currently carries only `set_leds`.

## 4. `usb-hid` device tree link (needed by P4)

The USB core has no device tree node of its own, so the `uses` declarations of
`usb-hid` need a link chain. `drivers/usb-hid/interfaces/imports.if` is comment-only
until this is answered.

## 5. Scope beyond RPi3 (needed before any RPi4 work)

RPi4's USB-A ports sit on a VL805 xHCI behind PCIe. All work so far targets the
RPi3 DWC2 controller. The xHCI follow-up is recorded in the design doc. The question
is whether to plan it now, and whether the `usb_hcd_v1` interface must stay neutral
for it. The DMA decision above applies to xHCI rings too.
