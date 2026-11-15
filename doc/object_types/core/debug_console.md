# DebugConsole

| | |
|---|---|
| Wire type | `0x7f` (core) |
| Backing | None |
| Operations | Write |
| Status | Debug kernels only |

## Purpose

DebugConsole prints text for debugging. It exists only in kernels built with the `debug_kernel` Cargo feature. There the boot code holds it at `KeySlot::DEBUG_CONSOLE` (slot 15), and every component Kickstart loads gets a `WRITE` copy in the same slot of its own table, also named by `DriverInit::debug_console`. Other kernels have no DebugConsole capability. It is meant for debugging; production components should not depend on it.

The userspace runtime's panic handler prints the panic message on it (`libobject::debug_console::set_panic_console`; `libdevice_driver::driver_init` sets it from `DriverInit::debug_console`).

## Operations

| Op | Name | Arguments | Rights | Result |
|---|---|---|---|---|
| 0 | Write | `x2` buffer address, `x3` length | — | — |

### Write

Prints the bytes at `x2` as they are, with no newline added. `DebugConsoleKey::write(&[u8])` wraps it.

- `x2` is a virtual address in the caller's AddressSpace. Every page of the range must be readable at the caller's exception level: EL0 for a component, EL1 for the boot code.
- At most `libobject::debug_console::WRITE_LIMIT` (1024) bytes are printed; the rest of a longer buffer is dropped without an error.
- The bytes are copied before they are printed.
- Under QEMU the output goes to the semihosting console, up to the first NUL byte. On the boards it goes to the console UART, with each malformed UTF-8 sequence printed as U+FFFD.

| Error | When |
|---|---|
| `InvalidPointer` | A page of the range is not readable by the caller, or the range wraps past the top of the address space; nothing is printed |
| `InvalidOperation` | The operation is not Write |

## Implementation

DebugConsole capabilities can be copied with [`KeyTable.CopyDerive`](key_table.md#copyderive), so a component can pass the console on to the components it builds.
