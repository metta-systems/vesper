_default:
    @just --list

# Clean project
clean:
    cargo make clean

# Update all dependencies
deps-up:
    cargo update

# Build default hw kernel and run chainofcommand to boot this kernel onto the board
boot: chainofcommand
    cargo make chainboot # make boot-kernel ?

# Build and run kernel in QEMU with serial port emulation
zellij:
    cargo make zellij-nucleus
    zellij --layout emulation/layout.zellij

# Build and run chainboot in QEMU with serial port emulation
zellij-cb:
    # Connect to it via chainofcommand to load an actual kernel
    # TODO: actually run chainofcommand in a zellij session too
    cargo make zellij-cb
    zellij --layout emulation/layout.zellij

# Build chainofcommand serial loader
chainofcommand:
    cd bin/chainofcommand
    cargo make build # --workspace=bin/chainofcommand

# Build and run kernel in QEMU
qemu:
    cargo make qemu

# Build and run kernel in QEMU with GDB port enabled
qemu-gdb:
    cargo make qemu-gdb

# Build and run chainboot in QEMU
qemu-cb:
    # Connect to it via chainofcommand to load an actual kernel
    cargo make qemu-cb

# Build and run chainboot in QEMU with GDB port enabled
qemu-cb-gdb:
    # Connect to it via chainofcommand to load an actual kernel
    cargo make qemu-cb-gdb

# Build and write kernel to an SD Card
device:
    cargo make sdcard

# Build and write kernel to an SD Card, then eject the SD Card volume
device-eject:
    cargo make sdeject

# Build and write chainboot to an SD Card, then eject the SD Card volume
cb-eject:
    cd bin/chainboot
    cargo make cb-eject

# Build default hw kernel
build:
    cargo make build-device
    cargo make kernel-binary # Should be only one command to do that, not two!

# Run clippy checks
clippy:
    # TODO: use cargo-hack
    cargo make xtool-clippy
    env CLIPPY_FEATURES=noserial cargo make xtool-clippy
    env CLIPPY_FEATURES=qemu cargo make xtool-clippy
    env CLIPPY_FEATURES=noserial,qemu cargo make xtool-clippy
    env CLIPPY_FEATURES=jtag cargo make xtool-clippy
    env CLIPPY_FEATURES=noserial,jtag cargo make xtool-clippy

# Run tests in QEMU
test:
    cargo make test

alias disasm := hopper

# Build and disassemble kernel
hopper:
    cargo make xtool-hopper

alias ocd := openocd

# Start openocd (by default connected via JTAG to a target device)
openocd:
    cargo make openocd

# Build and run kernel in GDB using openocd or QEMU as target (gdb port 5555)
gdb:
    cargo make gdb

# Build and run chainboot in GDB using openocd or QEMU as target (gdb port 5555)
gdb-cb:
    cargo make gdb-cb

# Build and print all symbols in the kernel
nm:
    cargo make xtool-nm

# Run `cargo expand` on nucleus
expand:
    cargo make xtool-expand-target -- nucleus

# Render modules dependency tree
modules:
    cargo make xtool-modules

# Generate and open documentation
doc:
    cargo make docs-flow

# Check formatting
fmt-check:
    cargo fmt -- --check

# Run lint tasks
lint: clippy fmt-check

# Run CI tasks
ci: clean build test lint
