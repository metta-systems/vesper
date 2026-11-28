# Multi-display implementation plan

Open tasks for the multi-display work, in order. The rules are in
[multi-display-design.md](multi-display-design.md), the open questions in
[multi-display-decisions.md](multi-display-decisions.md), finished tasks in
[multi-display-completed.md](multi-display-completed.md).

## Raspberry Pi 5 HDMI

- [ ] **MULTI-DISPLAY-17: the firmware's own framebuffer size.** The attributes
  request reads the physical size back (Get physical size, `0x00040003`),
  cross-checked with the pitch and the allocation; the fb driver configures and
  lists that size as its only mode, and refuses `set_mode` to any other. Host
  tests for the request and the check; the board check is the Pi 5's 1920x1080
  filling the screen.

## P-MULTI-DISPLAY-1: EDID over DDC on the Raspberry Pi 5

- [ ] Its firmware answers Get EDID block with `0x80000001` (request not
  understood), as it no longer drives HDMI. A `brcm,brcmstb-i2c` driver for
  the HDMI DDC buses (`ddc0`/`ddc1`, linked from `hdmi0`/`hdmi1` by `ddc`)
  reads the EDID blocks from address 0x50 (VESA E-DDC) and gives them to the
  fb driver, so its mode list is the monitor's.
  - First find out what the firmware sends to a monitor that cannot show
    1920x1080 (a scaled native mode?), and whether any firmware setting
    (`config.txt`) makes the framebuffer the monitor's size, since the mailbox
    does not. The answer decides what a listed mode can configure.

## Waiting for Q-MULTI-DISPLAY-1 and Q-MULTI-DISPLAY-2

Work on these stopped on 2026-11-25 by the maintainer's choice; it resumes by
discussing both questions in detail. Nothing in use needs them today: no output
changes by itself (the mailbox framebuffer has no hotplug source, since the
firmware does not report HDMI plug events, and the panel is fixed), and the
only layout client, the shell's `layout` command, reads on demand.

- [ ] **MULTI-DISPLAY-9 and MULTI-DISPLAY-10: hotplug plumbing** (after
  Q-MULTI-DISPLAY-1): provider EventCounts and the generation path, plus the
  arrival and removal policy, tested with a fake provider that comes and goes
  (`not_present`). The policy's core is built and host-tested
  (`Layout::rehome`, `Layout::renormalize`, `State::apply_layout`), but nothing
  triggers it yet. The fb driver also needs a source of change of its own:
  polling the firmware for the display's presence and EDID.
- [ ] **MULTI-DISPLAY-9: the layout EventCount** (after Q-MULTI-DISPLAY-2): it
  tells layout readers that the arrangement changed.
