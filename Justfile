_default:
    @just --list

# Build and run kernel in QEMU with serial port emulation
zellij:
    cargo make zellij-nucleus
    zellij --layout-path emulation/layout.zellij

# Build and run kernel in QEMU
qemu:
    cargo make qemu

# Build and run kernel in QEMU with GDB port enabled
qemu-gdb:
    cargo make qemu-gdb

# Build and write kernel to an SD Card
device:
    cargo make sdcard

# Build and write kernel to an SD Card, then eject the SD Card volume
device-eject:
    cargo make sdeject

# Build default hw kernel
build:
    cargo make build

# Clean project
clean:
    cargo make clean

# Run clippy checks
clippy:
    # TODO: use cargo-hack
    cargo make clippy
    env CLIPPY_FEATURES=noserial cargo make clippy
    env CLIPPY_FEATURES=qemu cargo make clippy
    env CLIPPY_FEATURES=noserial,qemu cargo make clippy
    env CLIPPY_FEATURES=jtag cargo make clippy
    env CLIPPY_FEATURES=noserial,jtag cargo make clippy

# Run tests in QEMU
test:
    cargo make test

alias disasm := hopper

# Build and disassemble kernel
hopper:
    cargo make hopper

alias ocd := openocd

# Start openocd (by default connected via JTAG to a target device)
openocd:
    cargo make openocd

# Build and run kernel in GDB using openocd or QEMU as target (gdb port 5555)
gdb:
    cargo make gdb

# Build and print all symbols in the kernel
nm:
    cargo make nm

# Check formatting
fmt-check:
    cargo fmt -- --check

# Run `cargo expand` on nucleus
expand:
    cargo make expand -- nucleus

# Generate and open documentation
doc:
    cargo make docs-flow

# Run CI tasks
ci: clean build test clippy fmt-check
