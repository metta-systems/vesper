# Input event completed

Short markers of what is done. Each line says what exists and where it is exercised.
Open items stay in [input-implementation-plan.md](input-implementation-plan.md) and
[input-decisions.md](input-decisions.md).

## Design

- [x] Event contract design written: record, descriptor,
  frames, overflow and consumer ends. `doc/agentic/input-design.md`.

## INPUT0: event contract

- [x] Event record, kinds, code namespace, framing, channel granularity, output direction, timestamp placement and hotplug id rules decided (D1 to D4.1, D7, D7.1, D11, D12, D13) and written into `input-design.md`. The descriptor layout is defined in INPUT1.

## INPUT1: event library

- [x] `libs/input-event` (`vesper-input-event`): the 24-byte `Event` with checked decode (length, kind, reserved bytes, key values), usage constants for the pages in use, the 272-byte `Descriptor` with strict decode, and a `State` tracker (key and button bitmaps, absolute axes, stale mark, release on `DeviceRemoved`). Wire layouts are pinned by literal-byte tests. Exercised by `just test-input-host`, linted by `just clippy-input-host`.
