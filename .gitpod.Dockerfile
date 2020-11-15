FROM gitpod/workspace-full

USER gitpod

RUN sudo apt-get update \
    && sudo apt-get install -y --no-install-recommends \
        pkg-config \
        libpython3.6 \
        rust-lldb \
        qemu \
        gdb \
    && .cargo/bin/rustup target add aarch64-linux-android \
    && .cargo/bin/rustup component add clippy llvm-tools-preview rls rust-analysis rustfmt rust-src \
    && .cargo/bin/cargo install cargo-bloat cargo-asm cargo-expand cargo-graph \
        cargo-binutils cargo-geiger cargo-cache cargo-make just \
    && .cargo/bin/cargo cache -i \
    && .cargo/bin/cargo cache -e \
    && .cargo/bin/cargo cache -i \
    && sudo rm -rf /var/lib/apt/lists/*

ENV RUST_LLDB=/usr/bin/lldb-8
