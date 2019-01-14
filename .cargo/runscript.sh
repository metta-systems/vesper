#!/bin/sh

aarch64-unknown-linux-musl-objcopy -O binary $1 $1.bin

# -d in_asm,unimp,int -S
/usr/local/Cellar/qemu/HEAD-3365de01b5-custom/bin/qemu-system-aarch64 -M raspi3 -serial null -serial stdio -kernel $1.bin
