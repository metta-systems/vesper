#!/bin/sh

aarch64-unknown-linux-musl-objcopy -O binary $1 $1.bin

# -d unimp,int -serial stdio -S
qemu-system-aarch64 -M raspi3 -d in_asm -serial null -serial stdio -kernel $1.bin
