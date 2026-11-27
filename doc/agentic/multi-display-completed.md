# Multi-display completed

One line per finished task. The rules are in
[multi-display-design.md](multi-display-design.md).

- [x] `display_v1` on the panel: Meddle's `MAX_DATA_BYTES` at 256 KiB, `flush` from the data area, `rotate` and the single mode; panel-test's client writes RGB565 and flushes rectangles on the board; host tests for the rectangle-to-window commands.
- [x] `display_v1` on the fb driver, next to `fb_v1`: host-tested EDID parsing (`fb_impl::edid`), `modes`, `get_mode`/`set_mode`, the self-configured preferred mode and `frames`.
- [x] `via every` in Meddle, image-build and Kickstart, with host tests; every-test (QEMU) has three exporters of `echo_v1`.
- [x] Multi-output compositor core: conversion, viewports, pointer union, placement and renormalization in `compositor-core` with host tests; the output table, per-output composition and the presenter Thread; HDMI and the panel together on the board.
- [x] `display_layout_v1`, `compositor_v1.display_layout`, the manifest flag and badge bit; layout-test passes under QEMU, the shell's `layout` commands on the board.
- [x] `rgb565` (little-endian) in `display_v1` (P3): the fb driver takes the pixel size from the pitch and logs the firmware's depth beside it; the compositor converts to it (`compositor-core` host test); the Pi 5's 16-bit HDMI framebuffer shows wood on the board.
- [x] Pi 5 framebuffer memory: `/reserved-memory` and `simple-framebuffer` nodes are no devices (`vesper-devicetree` tests, `targets/firmware-framebuffer-test.dtb`), and Kickstart's video memory covers the firmware's framebuffer node.
