#!/bin/sh
cargo xbuild --target=targets/aarch64-vesper-metta.json --release && \
sh .cargo/runscript.sh target/aarch64-vesper-metta/release/vesper
