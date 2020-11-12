qemu:
    # Build and run kernel in QEMU
    cargo make qemu

device:
    # Build and write kernel to an SD Card
    cargo make sdcard

build:
    # Build default hw kernel
    cargo make build

clean:
    # Clean project
    cargo make clean
    rm -f kernel8 kernel8.img

clippy:
    # Run clippy checks
    cargo make clippy

test:
    # Run tests in QEMU
    cargo make test

alias disasm := hopper

hopper:
    # Build and disassemble kernel
    cargo make hopper

alias ocd := openocd

openocd:
    # Start openocd (by default connected via JTAG to a target device)
    cargo make openocd

gdb:
    # Build and run kernel in GDB using openocd or QEMU as target (gdb port 5555)
    cargo make gdb

nm:
    # Build and print all symbols in the kernel
    cargo make nm
