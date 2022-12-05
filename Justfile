# Build and run kernel in QEMU
qemu:
    cargo make qemu

# Build and run kernel in QEMU with GDB port enabled
qemu-gdb:
    cargo make qemu-gdb

# Build default hw kernel
build:
    cargo make build-device
    cargo make kernel-binary

# Clean project
clean:
    cargo make clean

# Build and run kernel in GDB using openocd or QEMU as target (gdb port 5555)
gdb:
    cargo make gdb
