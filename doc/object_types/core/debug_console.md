# DebugConsole

| | |
|---|---|
| Wire type | `0x7f` (core) |
| Backing | None |
| Status | Write; debug kernels only |

## Purpose

DebugConsole prints text for debugging. It exists only in kernels built with the `debug_kernel` Cargo feature, where the first component receives it at `KeySlot::DEBUG_CONSOLE` (slot 15). Other kernels have no DebugConsole capability. It is meant for trusted test code; production components should not depend on it.

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Write | `x2` buffer address, `x3` length | — | — |

### Write

Prints `x3` bytes starting at `x2`. `DebugConsoleKey::write(&str)` wraps it.

- `x2` is read as a physical address. Write works only for buffers whose virtual address equals their physical address, as in the test kernels' images.
- The length must be below 4096 bytes; a longer length halts the kernel.
- The bytes must not contain a NUL byte; if they do, Write fails with `UNKNOWN`.
- Output goes to the QEMU semihosting console and appears only when the kernel runs under QEMU.

## Implementation

DebugConsole capabilities can be copied with [`KeyTable.CopyDerive`](key_table.md#copyderive), so a component can pass the console on to the components it builds.
