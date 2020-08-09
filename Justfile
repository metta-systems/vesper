qemu:
    cargo make qemu

device:
    cargo make sdcard

build:
    # Build default hw kernel
    cargo make build

clean:
    cargo make clean
    rm -f kernel8 kernel8.img

clippy:
    cargo make clippy
