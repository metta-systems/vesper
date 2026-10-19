# USB completed

Short markers of what is done, grouped by plan phase. Each line says what exists and where it is exercised. Rationale and details are in the [design](usb-design.md); the history of how each piece landed is in version control.

## USB0: design and interfaces

- [x] Interfaces `usb_hcd_v1` (`drivers/usb-hcd-dwc2/interfaces`), `usb_v1` (`drivers/usb/interfaces`) and `input_v1` (`drivers/usb-hid/interfaces`) parse under `meddler check`.

## USB1: pure protocol logic

- [x] `drivers/usb/protocol` (`vesper-usb-protocol`): setup packets, descriptor walker, enumeration state machine, HID boot-report decoder; no I/O, no allocation. Exercised by `just test-usb-host`, linted by `just clippy-usb-host`.

## USB2: DWC2 root port and EP0

- [x] `drivers/usb-hcd-dwc2`: register map on `bitbybit` fields and a `derive_mmio` block, the control-stage plan, and an EL0 component that resets the root port and runs EP0 control transfers by DMA through a driver-owned, cache-maintained page. Exercised by `just test-usb` (QEMU raspi3b, hub on the root port, reported as class 9 at address 1).
- [x] Only the root device is enumerated (by the client, over `usb_hcd_v1`).

## Shared changes

- [x] `libs/device-driver`: `MAX_DEVICE_PAGES` is 32 (Pi 3 DWC2 node: 64 KiB window plus a 4 KiB range). Exercised by `just test-driver`.
- [x] `libs/serial-channel`: `CONTROL_VA` is `0x3802_0000`, above the 32 register pages. Exercised by `just test-uart`.
- [x] `Justfile`: `usb-hcd-dwc2` and `usb-test-client` in `user_components`, `usb-hcd-dwc2` in `driver_components`; recipes `test-usb-host`, `clippy-usb-host`, `build-usb-test`, `test-usb`. The USB crates, the generated `usb-hcd` and `usb-hcd-client` and the test client are in the `Cargo.toml` workspace.
- [x] `libs/meddle-runtime`: `DataArea::read_bytes` and `write_bytes` for byte-granular access to a shared data area. Exercised by `just test-meddle` and `just test-usb`.
- [x] `drivers/usb/protocol`: `SetupPacket::from_bytes`, the inverse of `to_bytes`. Exercised by `just test-usb-host`.

## USB2 follow-ups

- [x] `usb_hcd_v1` and `pipe_v1` control pipes served by `drivers/usb-hcd-dwc2` (generated `server` and `client` crates): `reset_port`, `port_status`, `open_pipe` (control only; installs the four `pipe_v1` keys `set_route` (stored), `close_pipe`, `clear_halt`, `control` in the caller's lent table). Pipes are a 16-entry table of generation-checked handles, the badge of the pipe's keys: holding a key is the right to use the pipe. Exercised by `just test-usb` (`usb-test-client` enumerates the hub through the import and checks refusals) and the host tests of `pipes`, `control::data_length` and `port_status_bits` (`just test-usb-host`).
- [x] Interrupt-driven completion in `drivers/usb-hcd-dwc2`: the component's Thread services the controller's interrupt (`HAINT`, `GINTMSK`, `HAINTMSK` and the channel masks in `regs.rs`) and advances a `completions` EventCount that control transfers sleep on, with a transfer deadline. Exercised by `just test-usb` (the client enumerates the hub; about 20 interrupts per run) and the register tests of `just test-usb-host`.
