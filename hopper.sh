#!/bin/sh
hopperv4 -e target/aarch64-vesper-metta/release/vesper.bin -R --base-address 0x80000 --entrypoint 0x80000 --file-offset 0 --aarch64
