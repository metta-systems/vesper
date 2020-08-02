qemu:
    cargo make qemu

device:
    cargo make sdcard

clean:
    cargo make clean
    rm -f kernel8 kernel8.img

