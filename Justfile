_default:
    @just --list

# Update all dependencies
[group("maintenance")]
deps-up:
    cargo update

# Build default hw kernel and run chainofcommand to boot this kernel onto the board
[group("hw")]
boot: chainofcommand
    cargo make chainboot # make boot-kernel ?

# Build and run kernel in QEMU with serial port emulation
[group("emu")]
zellij:
    cargo make zellij-nucleus
    zellij --layout emulation/layout.zellij

# Build and run chainboot in QEMU with serial port emulation
[group("emu")]
zellij-cb:
    # Connect to it via chainofcommand to load an actual kernel
    # TODO: actually run chainofcommand in a zellij session too
    cargo make zellij-cb
    zellij --layout emulation/layout.zellij

# Build chainofcommand serial loader
[group("hw")]
chainofcommand:
    cd bin/chainofcommand
    cargo make build # --workspace=bin/chainofcommand

# Build and run kernel in QEMU
[group("emu")]
qemu:
    cargo make qemu

# Build and run kernel in QEMU with GDB port enabled
[group("emu")]
qemu-gdb:
    cargo make qemu-gdb

# Build and run chainboot in QEMU
[group("emu")]
qemu-cb:
    # Connect to it via chainofcommand to load an actual kernel
    cargo make qemu-cb

# Build and run chainboot in QEMU with GDB port enabled
[group("emu")]
qemu-cb-gdb:
    # Connect to it via chainofcommand to load an actual kernel
    cargo make qemu-cb-gdb

# Build and write kernel to an SD Card
[group("hw")]
device:
    cargo make sdcard

# Build and write kernel to an SD Card, then eject the SD Card volume
[group("hw")]
device-eject:
    cargo make sdeject

# Build and write chainboot to an SD Card, then eject the SD Card volume
[group("hw")]
cb-eject:
    cd bin/chainboot
    cargo make cb-eject

# Build default hw kernel
[group("hw")]
build:
    cargo make build

# Build default hw kernel (quietly)
[group("hw")]
qbuild:
    cargo make build

alias b := build

# Clean project
[group("maintenance")]
clean:
    cargo make clean

# Run clippy checks
[group("maintenance")]
clippy:
    # TODO: use cargo-hack
    cargo make clippy
    env CLIPPY_FEATURES=noserial cargo make clippy
    env CLIPPY_FEATURES=qemu cargo make clippy
    env CLIPPY_FEATURES=noserial,qemu cargo make clippy
    env CLIPPY_FEATURES=jtag cargo make clippy
    env CLIPPY_FEATURES=noserial,jtag cargo make clippy

# Run shortened clippy checks
[group("maintenance")]
clippy-pre-push:
    cargo make clippy

# Run tests in QEMU
[group("emu")]
test:
    cargo make test

alias disasm := hopper

# Build and disassemble kernel
[group("debug")]
hopper:
    cargo make xtool-hopper

alias ocd := openocd

# Start openocd (by default connected via JTAG to a target device)
[group("hw")]
openocd:
    cargo make openocd

# Build and run kernel in GDB using openocd or QEMU as target (gdb port 5555)
[group("debug")]
gdb:
    cargo make gdb

# Build and run chainboot in GDB using openocd or QEMU as target (gdb port 5555)
[group("debug")]
gdb-cb:
    cargo make gdb-cb

# Build and print all symbols in the kernel
[group("maintenance")]
nm:
    cargo make xtool-nm

# Run `cargo expand` on nucleus
[group("maintenance")]
expand:
    cargo make xtool-expand-target -- nucleus

# Render modules dependency tree
[group("maintenance")]
modules:
    cargo make xtool-modules

# Generate and open documentation
[group("maintenance")]
doc:
    cargo make docs-flow

# Check formatting
[group("maintenance")]
fmt-check:
    cargo fmt -- --check

# Run lint tasks
[group("maintenance")]
lint: clippy fmt-check

# Run pre-push local checks
[group("ci")]
pre-push: fmt-check clippy-pre-push test

# Run CI tasks
[group("ci")]
ci: clean build test lint

# Prepare local dev tools and set-up git hooks
[group("maintenance")]
setup-local-dev:
    commit-emoji --help || cargo install commit-emoji
    commit-emoji -i
    # Run local shortened clippy before pushing to remote
    cp .hooks/pre-push .git/hooks/pre-push
