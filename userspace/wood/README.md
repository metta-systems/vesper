A Doom port based on [Room](./room/README.md).

Version for vesper;

- link in libroom and then
- setup a vesper app component from here
- wire up compositor, events,
- load WAD from resource

requires:
- memory allocator ([rlsf](https://github.com/yvt/rlsf/tree/main/crates/rlsf))
- memio implementation to read from WAD resource

Phase 1:
- create compositor window
- load WAD
- play autodemo
